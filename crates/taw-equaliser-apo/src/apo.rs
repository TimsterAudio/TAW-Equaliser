//! The engine's logic: format checks, starting and stopping the stream,
//! and the real-time processing. `com.rs` exposes it to Windows.
//!
//! Call order from the audio engine: `Initialize` once, format checks,
//! `LockForProcess` with the final format, then `APOProcess` on the
//! real-time thread for every buffer, and `UnlockForProcess` when the
//! stream stops. Nothing here may panic: a panic would take down the
//! Windows audio engine.

use crate::chain::{self, Chained};
use crate::events::{self, Level};
use crate::settings_watch;
use crate::status;
use crate::CLSID_EQ_BOX_APO;
use eq_core::dsp::Equalizer;
use eq_core::shared::Shared;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{E_INVALIDARG, E_OUTOFMEMORY, E_POINTER, HANDLE};
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
use windows::Win32::System::Com::CoTaskMemAlloc;
use windows_core::{Error, Interface, Result, GUID};

// ---------------------------------------------------------------- engine

#[derive(Clone, Copy, PartialEq)]
struct Format {
    channels: u32,
    rate: f32,
}

struct State {
    initialized: bool,
    locked: bool,
    format: Option<Format>,
    eq: Option<Equalizer>,
    /// The device's own effect, run after the EQ (see `chain.rs`).
    chained: Option<Chained>,
}

/// The engine's state and logic, without the COM plumbing (see `com.rs`).
pub struct Engine {
    state: Mutex<State>,
    shared: Arc<Shared>,
    /// Whether to run the device's own effect after the EQ.
    chains: bool,
}

impl Engine {
    pub fn new(aggregated: bool, chains: bool) -> Self {
        let n = status::CREATED.fetch_add(1, Relaxed) + 1;
        events::log(
            Level::Debug,
            &format!(
                "engine instance {n} created ({})",
                if aggregated {
                    "aggregated by the audio engine"
                } else {
                    "standalone"
                }
            ),
        );
        Engine {
            state: Mutex::new(State {
                initialized: false,
                locked: false,
                format: None,
                eq: None,
                chained: None,
            }),
            shared: settings_watch::shared(),
            chains,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // Nothing panics while holding the lock, but never let a poisoned
        // lock stop the audio.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The format of a media type, if it's one the Equaliser can run on: uncompressed
/// 32-bit float, which is what the Windows audio engine uses internally.
fn float_format(t: &IAudioMediaType) -> Result<Format> {
    let mut f = UNCOMPRESSEDAUDIOFORMAT::default();
    unsafe { t.GetUncompressedAudioFormat(&mut f)? };
    let is_float = f.guidFormatType == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        && f.dwBytesPerSampleContainer == 4
        && f.dwValidBitsPerSample == 32;
    if !is_float || f.dwSamplesPerFrame == 0 || f.fFramesPerSecond <= 0.0 {
        status::refused(
            f.dwValidBitsPerSample,
            f.dwBytesPerSampleContainer,
            f.guidFormatType == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        );
        events::log_limited(
            &status::FORMATS_REFUSED_LOGGED,
            10,
            Level::Warning,
            &format!(
                "refused format: {:?} {}-bit in {}-byte container, {} ch, {} Hz",
                f.guidFormatType,
                f.dwValidBitsPerSample,
                f.dwBytesPerSampleContainer,
                f.dwSamplesPerFrame,
                f.fFramesPerSecond
            ),
        );
        return Err(APOERR_FORMAT_NOT_SUPPORTED.into());
    }
    Ok(Format {
        channels: f.dwSamplesPerFrame,
        rate: f.fFramesPerSecond,
    })
}

/// The Equaliser works in place, so input and output must be the same format.
fn check_format(
    opposite: Option<&IAudioMediaType>,
    requested: Option<&IAudioMediaType>,
) -> Result<IAudioMediaType> {
    status::FORMAT_QUERIES.fetch_add(1, Relaxed);
    let req = requested.ok_or(Error::from(E_POINTER))?;
    let f = float_format(req)?;
    if let Some(opp) = opposite {
        let o = float_format(opp)?;
        if o != f {
            events::log_limited(
                &status::FORMATS_REFUSED_LOGGED,
                10,
                Level::Warning,
                &format!(
                    "refused format: input and output differ ({} ch {} Hz vs {} ch {} Hz)",
                    f.channels, f.rate, o.channels, o.rate
                ),
            );
            return Err(APOERR_FORMAT_NOT_SUPPORTED.into());
        }
    }
    events::log_limited(
        &status::FORMATS_ACCEPTED_LOGGED,
        5,
        Level::Debug,
        &format!(
            "accepted format: 32-bit float, {} ch, {} Hz",
            f.channels, f.rate
        ),
    );
    Ok(req.clone())
}

fn wide<const N: usize>(s: &str) -> [u16; N] {
    let mut out = [0u16; N];
    for (o, c) in out.iter_mut().zip(s.encode_utf16().take(N - 1)) {
        *o = c;
    }
    out
}

impl Engine {
    pub fn reset(&self) -> Result<()> {
        let mut st = self.state();
        if let Some(eq) = st.eq.as_mut() {
            eq.reset();
        }
        if let Some(c) = st.chained.as_ref() {
            c.reset();
        }
        Ok(())
    }

    pub fn latency(&self) -> i64 {
        self.state().chained.as_ref().map_or(0, Chained::latency)
    }

    /// A format query (`input` says which side). The device's own effect,
    /// if any, must take the format too; if it won't, the Equaliser drops it
    /// rather than stop the sound.
    pub fn check_format(
        &self,
        opposite: Option<&IAudioMediaType>,
        requested: Option<&IAudioMediaType>,
        input: bool,
    ) -> Result<IAudioMediaType> {
        let t = check_format(opposite, requested)?;
        let mut st = self.state();
        if let Some(c) = st.chained.as_ref() {
            if !c.accepts(opposite, &t, input) {
                events::log(
                    Level::Warning,
                    &format!(
                        "the device's own effect {:?} refused the format; running without it",
                        c.clsid
                    ),
                );
                st.chained = None;
            }
        }
        Ok(t)
    }

    pub fn registration_properties(&self) -> Result<*mut APO_REG_PROPERTIES> {
        let props = APO_REG_PROPERTIES {
            clsid: CLSID_EQ_BOX_APO,
            Flags: APO_FLAG(APO_FLAG_DEFAULT.0 | APO_FLAG_INPLACE.0),
            szFriendlyName: wide("TAW Equaliser"),
            szCopyrightInfo: wide("Timster's Audio Works"),
            u32MajorVersion: env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0),
            u32MinorVersion: env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0),
            u32MinInputConnections: 1,
            u32MaxInputConnections: 1,
            u32MinOutputConnections: 1,
            u32MaxOutputConnections: 1,
            u32MaxInstances: u32::MAX,
            u32NumAPOInterfaces: 1,
            iidAPOInterfaceList: [IAudioProcessingObject::IID],
        };
        // The caller frees this with CoTaskMemFree.
        let p = unsafe { CoTaskMemAlloc(core::mem::size_of::<APO_REG_PROPERTIES>()) }
            as *mut APO_REG_PROPERTIES;
        if p.is_null() {
            return Err(E_OUTOFMEMORY.into());
        }
        unsafe { p.write(props) };
        Ok(p)
    }

    pub fn initialize(&self, cbdatasize: u32, pbydata: *const u8) -> Result<()> {
        // The data is the device's property stores. the Equaliser's settings come
        // from its own file; the stores only say which effect to chain.
        let data: &[u8] = if pbydata.is_null() || cbdatasize == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(pbydata, cbdatasize as usize) }
        };
        if self.state().initialized {
            return Err(APOERR_ALREADY_INITIALIZED.into());
        }
        let chained = if self.chains {
            chain::from_setup_data(data)
        } else {
            None
        };
        let mut st = self.state();
        if st.initialized {
            return Err(APOERR_ALREADY_INITIALIZED.into());
        }
        st.initialized = true;
        st.chained = chained;
        status::INITIALIZED.fetch_add(1, Relaxed);
        events::log(
            Level::Debug,
            &format!("initialised ({cbdatasize} bytes of setup data)"),
        );
        Ok(())
    }

    pub fn input_channel_count(&self) -> Result<u32> {
        match self.state().format {
            Some(f) => Ok(f.channels),
            None => Err(APOERR_NOT_INITIALIZED.into()),
        }
    }

    pub fn lock_for_process(
        &self,
        u32numinputconnections: u32,
        ppinputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
        u32numoutputconnections: u32,
        ppoutputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
    ) -> Result<()> {
        // Counted, so the status report shows if Windows can't start us.
        let result = (|| -> Result<()> {
            if u32numinputconnections != 1 || u32numoutputconnections != 1 {
                return Err(APOERR_NUM_CONNECTIONS_INVALID.into());
            }
            if ppinputconnections.is_null() || ppoutputconnections.is_null() {
                return Err(E_POINTER.into());
            }
            let (input, output) = unsafe {
                let i = *ppinputconnections;
                let o = *ppoutputconnections;
                if i.is_null() || o.is_null() {
                    return Err(E_POINTER.into());
                }
                (&*i, &*o)
            };
            let in_fmt = float_format(input.pFormat.as_ref().ok_or(Error::from(E_INVALIDARG))?)?;
            let out_fmt = float_format(output.pFormat.as_ref().ok_or(Error::from(E_INVALIDARG))?)?;
            if in_fmt != out_fmt {
                return Err(APOERR_INVALID_CONNECTION_FORMAT.into());
            }

            let mut st = self.state();
            if !st.initialized {
                return Err(APOERR_NOT_INITIALIZED.into());
            }
            if st.locked {
                return Err(APOERR_APO_LOCKED.into());
            }
            // Allocating here is fine: this isn't the real-time thread.
            st.eq = Some(Equalizer::new(in_fmt.rate as f64, in_fmt.channels as usize));
            st.format = Some(in_fmt);
            if let Some(c) = st.chained.as_mut() {
                if let Err(e) = c.lock(output, in_fmt.channels as usize) {
                    events::log(
                        Level::Warning,
                        &format!(
                            "the device's own effect {:?} could not start the stream ({e}); running without it",
                            c.clsid
                        ),
                    );
                    st.chained = None;
                }
            }
            st.locked = true;
            status::LOCKED.fetch_add(1, Relaxed);
            status::RATE.store(in_fmt.rate as u32, Relaxed);
            status::CHANNELS.store(in_fmt.channels, Relaxed);
            events::log_limited(
                &status::STREAMS_LOGGED,
                3,
                Level::Info,
                &format!("stream started: {} ch, {} Hz", in_fmt.channels, in_fmt.rate),
            );
            Ok(())
        })();
        if let Err(e) = &result {
            status::LOCK_FAILURES.fetch_add(1, Relaxed);
            events::log(
                Level::Error,
                &format!("could not start the stream (LockForProcess): {e}"),
            );
        }
        result
    }

    pub fn unlock_for_process(&self) -> Result<()> {
        let mut st = self.state();
        if !st.locked {
            return Err(APOERR_ALREADY_UNLOCKED.into());
        }
        st.locked = false;
        if let Some(c) = st.chained.as_mut() {
            c.unlock();
        }
        status::LOCKED.fetch_sub(1, Relaxed);
        Ok(())
    }

    pub fn process(
        &self,
        u32numinputconnections: u32,
        ppinputconnections: *const *const APO_CONNECTION_PROPERTY,
        u32numoutputconnections: u32,
        ppoutputconnections: *mut *mut APO_CONNECTION_PROPERTY,
    ) {
        // Real-time thread: no allocation, no blocking, no panics.
        if u32numinputconnections < 1
            || u32numoutputconnections < 1
            || ppinputconnections.is_null()
            || ppoutputconnections.is_null()
        {
            return;
        }
        let (input, output) = unsafe {
            let i = *ppinputconnections;
            let o = *ppoutputconnections;
            if i.is_null() || o.is_null() {
                return;
            }
            (&*i, &mut *o)
        };
        output.u32ValidFrameCount = input.u32ValidFrameCount;
        output.u32BufferFlags = input.u32BufferFlags;
        let valid =
            input.u32BufferFlags == BUFFER_VALID && input.pBuffer != 0 && output.pBuffer != 0;
        if valid {
            status::BUFFERS.fetch_add(1, Relaxed);
        } else {
            status::SILENT_BUFFERS.fetch_add(1, Relaxed);
        }

        // Only ever contended if Windows breaks its own call rules; then
        // the audio passes through untouched rather than waiting.
        let Ok(mut guard) = self.state.try_lock() else {
            return;
        };
        let st = &mut *guard;
        let Some(eq) = st.eq.as_mut() else { return };
        let channels = eq.channels();
        if valid {
            let n = input.u32ValidFrameCount as usize * channels;
            unsafe {
                if input.pBuffer != output.pBuffer {
                    core::ptr::copy(input.pBuffer as *const f32, output.pBuffer as *mut f32, n);
                }
                let buf = core::slice::from_raw_parts_mut(output.pBuffer as *mut f32, n);
                let settings = self.shared.load();
                eq.process(buf, &settings);
            }
        }
        // The device's own effect sees silent buffers too (it may have a
        // tail to play out).
        if let Some(c) = st.chained.as_mut() {
            c.process(output, channels);
        }
    }

    pub fn effects_list(
        &self,
        ppeffectsids: *mut *mut GUID,
        pceffects: *mut u32,
        _event: HANDLE,
    ) -> Result<()> {
        // The Equaliser doesn't expose individual effects to Windows' settings UI.
        if ppeffectsids.is_null() || pceffects.is_null() {
            return Err(E_POINTER.into());
        }
        if let Some(r) = self
            .state()
            .chained
            .as_ref()
            .and_then(|c| c.effects_list(ppeffectsids, pceffects, _event))
        {
            return r;
        }
        unsafe {
            *ppeffectsids = core::ptr::null_mut();
            *pceffects = 0;
        }
        Ok(())
    }
}
