//! The device's own effect, kept running after the Equaliser's EQ.
//!
//! Many sound card drivers put their own effect (speaker tuning,
//! "enhancements") in the slot the Equaliser installs into. Setup doesn't throw it
//! away: it records its CLSID in the device's effect settings under the Equaliser's
//! own key, and the engine creates that effect itself and runs the sound
//! through it after the EQ. If the effect can't be created or refuses to
//! start, the Equaliser carries on without it and says so in the event log.

use crate::events::{self, Level};
use crate::CLSID_EQ_BOX_APO;
use core::mem::{size_of, ManuallyDrop};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows_core::{Interface, Result, GUID};

/// Where setup records the effect the Equaliser replaced, in the device's effect
/// settings (`FxProperties`). Never change it: installed systems use it.
pub const PKEY_CHAINED_EFFECT: PROPERTYKEY = PROPERTYKEY {
    fmtid: CLSID_EQ_BOX_APO,
    pid: 1,
};

pub struct Chained {
    pub clsid: GUID,
    apo: IAudioProcessingObject,
    rt: IAudioProcessingObjectRT,
    cfg: IAudioProcessingObjectConfiguration,
    effects: Option<IAudioSystemEffects2>,
    /// Its output buffer (it may not work in place). Sized at lock time.
    scratch: Vec<f32>,
    out_desc: Option<APO_CONNECTION_DESCRIPTOR>,
    locked: bool,
}

/// The effect setup recorded for this device, read from the setup data the
/// audio engine passes to `Initialize`.
pub fn recorded_clsid(data: &[u8]) -> Option<GUID> {
    if data.len() < size_of::<APOInitSystemEffects>() {
        return None;
    }
    // The data starts with APOInitSystemEffects (or a larger version of it).
    let init = unsafe { &*(data.as_ptr() as *const APOInitSystemEffects) };
    let store = init.pAPOSystemEffectsProperties.as_ref()?;
    let mut value: PROPVARIANT = unsafe { store.GetValue(&PKEY_CHAINED_EFFECT) }.ok()?;
    let text = unsafe {
        let v = &value.Anonymous.Anonymous;
        if v.vt == VT_LPWSTR && !v.Anonymous.pwszVal.is_null() {
            v.Anonymous.pwszVal.to_string().ok()
        } else {
            None
        }
    };
    let _ = unsafe { PropVariantClear(&mut value) };
    parse_guid(&text?)
}

/// "{0FEBC01A-...}" or without the braces.
pub fn parse_guid(text: &str) -> Option<GUID> {
    let t = text.trim().trim_start_matches('{').trim_end_matches('}');
    GUID::try_from(t).ok()
}

impl Chained {
    /// Creates and initialises the effect with the same setup data the Equaliser
    /// was given, so it sees the same device and processing mode.
    pub fn create(clsid: GUID, data: &[u8]) -> Result<Chained> {
        let apo: IAudioProcessingObject =
            unsafe { CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER)? };
        // Its own copy of the setup data, naming its own CLSID.
        let mut own = data.to_vec();
        if own.len() >= size_of::<APOInitBaseStruct>() {
            let base = own.as_mut_ptr() as *mut APOInitBaseStruct;
            unsafe { core::ptr::addr_of_mut!((*base).clsid).write_unaligned(clsid) };
        }
        unsafe { apo.Initialize(&own)? };
        Ok(Chained {
            clsid,
            rt: apo.cast()?,
            cfg: apo.cast()?,
            effects: apo.cast().ok(),
            apo,
            scratch: Vec::new(),
            out_desc: None,
            locked: false,
        })
    }

    /// Whether it accepts `requested` unchanged.
    pub fn accepts(
        &self,
        opposite: Option<&IAudioMediaType>,
        requested: &IAudioMediaType,
        input: bool,
    ) -> bool {
        let r = unsafe {
            if input {
                self.apo.IsInputFormatSupported(opposite, requested)
            } else {
                self.apo.IsOutputFormatSupported(opposite, requested)
            }
        };
        // It may answer with a different format it would rather have;
        // The Equaliser needs it to take the format as it is.
        match r {
            Ok(t) => {
                t.as_raw() == requested.as_raw()
                    || unsafe { t.IsEqual(requested) }
                        .map(|f| {
                            f & AUDIOMEDIATYPE_EQUAL_FORMAT_TYPES != 0
                                && f & AUDIOMEDIATYPE_EQUAL_FORMAT_DATA != 0
                        })
                        .unwrap_or(false)
            }
            Err(_) => false,
        }
    }

    /// Starts it on the Equaliser's output, writing into its own buffer.
    pub fn lock(&mut self, output: &APO_CONNECTION_DESCRIPTOR, channels: usize) -> Result<()> {
        self.scratch = vec![0.0; output.u32MaxFrameCount as usize * channels];
        let desc = APO_CONNECTION_DESCRIPTOR {
            Type: APO_CONNECTION_BUFFER_TYPE_EXTERNAL,
            pBuffer: self.scratch.as_mut_ptr() as usize,
            u32MaxFrameCount: output.u32MaxFrameCount,
            pFormat: ManuallyDrop::new((*output.pFormat).clone()),
            u32Signature: output.u32Signature,
        };
        let input: *const APO_CONNECTION_DESCRIPTOR = output;
        let out: *const APO_CONNECTION_DESCRIPTOR = &desc;
        let result = unsafe { self.cfg.LockForProcess(&[input], &[out]) };
        if let Some(mut old) = self.out_desc.replace(desc) {
            unsafe { ManuallyDrop::drop(&mut old.pFormat) };
        }
        result?;
        self.locked = true;
        Ok(())
    }

    pub fn unlock(&mut self) {
        if self.locked {
            self.locked = false;
            let _ = unsafe { self.cfg.UnlockForProcess() };
        }
    }

    pub fn reset(&self) {
        let _ = unsafe { self.apo.Reset() };
    }

    pub fn latency(&self) -> i64 {
        unsafe { self.apo.GetLatency() }.unwrap_or(0)
    }

    /// Runs it on `conn` (the Equaliser's output) and copies its result back.
    /// Real-time: no allocation, no locks.
    pub fn process(&mut self, conn: &mut APO_CONNECTION_PROPERTY, channels: usize) {
        let n = conn.u32ValidFrameCount as usize * channels;
        if !self.locked || n > self.scratch.len() || conn.pBuffer == 0 {
            return;
        }
        let mut out = APO_CONNECTION_PROPERTY {
            pBuffer: self.scratch.as_mut_ptr() as usize,
            u32ValidFrameCount: conn.u32ValidFrameCount,
            u32BufferFlags: BUFFER_VALID,
            u32Signature: conn.u32Signature,
        };
        let input: *const APO_CONNECTION_PROPERTY = conn;
        let mut out_ptr: *mut APO_CONNECTION_PROPERTY = &mut out;
        unsafe {
            self.rt.APOProcess(1, &input, 1, &mut out_ptr);
            let dst = core::slice::from_raw_parts_mut(conn.pBuffer as *mut f32, n);
            if out.u32BufferFlags == BUFFER_SILENT {
                dst.fill(0.0);
            } else {
                dst.copy_from_slice(&self.scratch[..n]);
            }
        }
        conn.u32BufferFlags = out.u32BufferFlags;
        conn.u32ValidFrameCount = out.u32ValidFrameCount;
    }

    /// Its effects list, so Windows' sound settings still show them.
    pub fn effects_list(
        &self,
        ids: *mut *mut GUID,
        count: *mut u32,
        event: windows::Win32::Foundation::HANDLE,
    ) -> Option<Result<()>> {
        let e = self.effects.as_ref()?;
        Some(unsafe { e.GetEffectsList(ids, count, event) })
    }
}

impl Drop for Chained {
    fn drop(&mut self) {
        self.unlock();
        if let Some(mut d) = self.out_desc.take() {
            unsafe { ManuallyDrop::drop(&mut d.pFormat) };
        }
    }
}

/// Creates the recorded effect, if there is one, logging the outcome.
pub fn from_setup_data(data: &[u8]) -> Option<Chained> {
    let clsid = recorded_clsid(data)?;
    if clsid == CLSID_EQ_BOX_APO {
        return None;
    }
    match Chained::create(clsid, data) {
        Ok(c) => {
            events::log(
                Level::Info,
                &format!("running the device's own effect {clsid:?} after the EQ"),
            );
            Some(c)
        }
        Err(e) => {
            events::log(
                Level::Warning,
                &format!(
                    "the device's own effect {clsid:?} could not start ({e}); running without it"
                ),
            );
            None
        }
    }
}
