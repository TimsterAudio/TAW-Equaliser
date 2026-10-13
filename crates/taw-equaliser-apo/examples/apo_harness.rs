//! Loads `taw_equaliser_apo.dll` the way the Windows audio engine does, runs a
//! tone through it and checks the EQ, including a live settings change.
//!
//! cargo build --release -p taw-equaliser-apo --example apo_harness
//! then run apo_harness.exe from the folder holding taw_equaliser_apo.dll
//! (on Linux: `wine apo_harness.exe`). It writes the engine's settings file,
//! so don't run it on a machine where the engine is in use.

#[cfg(not(windows))]
fn main() {
    eprintln!("The APO harness only runs on Windows (or Wine).");
}

#[cfg(windows)]
fn main() {
    if let Err(e) = win::run() {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
    println!("PASS");
}

#[cfg(windows)]
mod win {
    use core::ffi::c_void;
    use eq_core::dsp::EqSettings;
    use eq_core::engine_settings;
    use std::f64::consts::PI;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::Win32::Foundation::{HMODULE, PROPERTYKEY};
    use windows::Win32::Media::Audio::Apo::*;
    use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemAlloc, CoTaskMemFree, IClassFactory, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Registry::{
        RegDeleteTreeW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, IPropertyStore_Impl};
    use windows_core::{
        implement, w, IUnknown, IUnknown_Vtbl, Interface, Ref, Result, BOOL, GUID, HRESULT, PCSTR,
    };
    use windows_core::{HSTRING, PCWSTR, PWSTR};

    const CLSID: GUID = GUID::from_u128(0x0febc01a_a301_491f_bbcd_17284a13cb61);
    const RATE: f32 = 48_000.0;
    const CH: u32 = 2;

    #[implement(IAudioMediaType)]
    struct FloatStereo;

    impl IAudioMediaType_Impl for FloatStereo_Impl {
        fn IsCompressedFormat(&self) -> Result<BOOL> {
            Ok(false.into())
        }
        fn IsEqual(&self, _other: Ref<IAudioMediaType>) -> Result<u32> {
            Ok(0)
        }
        fn GetAudioFormat(&self) -> *mut windows::Win32::Media::Audio::WAVEFORMATEX {
            core::ptr::null_mut()
        }
        fn GetUncompressedAudioFormat(&self, f: *mut UNCOMPRESSEDAUDIOFORMAT) -> Result<()> {
            unsafe {
                *f = UNCOMPRESSEDAUDIOFORMAT {
                    guidFormatType: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
                    dwSamplesPerFrame: CH,
                    dwBytesPerSampleContainer: 4,
                    dwValidBitsPerSample: 32,
                    fFramesPerSecond: RATE,
                    dwChannelMask: 3,
                };
            }
            Ok(())
        }
    }

    /// A stand-in for the audio engine's outer object when it aggregates
    /// an effect: it answers IUnknown itself and passes every other
    /// interface request to the effect's private IUnknown.
    #[repr(C)]
    struct Outer {
        vtbl: *const IUnknown_Vtbl,
        refs: AtomicU32,
        inner: *mut c_void,
    }

    unsafe extern "system" fn outer_query(
        this: *mut c_void,
        iid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT {
        unsafe {
            let o = &*(this as *const Outer);
            if *iid == IUnknown::IID {
                outer_add_ref(this);
                *ppv = this;
                return HRESULT(0);
            }
            let inner = &**(o.inner as *const *const IUnknown_Vtbl);
            (inner.QueryInterface)(o.inner, iid, ppv)
        }
    }

    unsafe extern "system" fn outer_add_ref(this: *mut c_void) -> u32 {
        unsafe { &*(this as *const Outer) }
            .refs
            .fetch_add(1, Ordering::SeqCst)
            + 1
    }

    unsafe extern "system" fn outer_release(this: *mut c_void) -> u32 {
        unsafe {
            let left = (*(this as *const Outer))
                .refs
                .fetch_sub(1, Ordering::SeqCst)
                - 1;
            if left == 0 {
                let o = Box::from_raw(this as *mut Outer);
                if !o.inner.is_null() {
                    let inner = &**(o.inner as *const *const IUnknown_Vtbl);
                    (inner.Release)(o.inner);
                }
            }
            left
        }
    }

    static OUTER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
        QueryInterface: outer_query,
        AddRef: outer_add_ref,
        Release: outer_release,
    };

    /// Creates the engine the way audiodg.exe does (aggregated), starts a
    /// stream and checks the EQ is applied.
    unsafe fn aggregated(factory: &IClassFactory) -> std::result::Result<(), String> {
        unsafe {
            let raw = Box::into_raw(Box::new(Outer {
                vtbl: &OUTER_VTBL,
                refs: AtomicU32::new(1),
                inner: core::ptr::null_mut(),
            }));
            let outer = IUnknown::from_raw(raw as *mut c_void);
            check(
                factory
                    .CreateInstance::<_, IAudioProcessingObject>(&outer)
                    .is_err(),
                "aggregated creation refuses anything but IUnknown",
            )?;
            let inner: IUnknown = factory
                .CreateInstance(&outer)
                .map_err(|e| format!("aggregated CreateInstance: {e}"))?;
            (*raw).inner = inner.into_raw();

            let apo: IAudioProcessingObject = outer
                .cast()
                .map_err(|e| format!("outer QueryInterface: {e}"))?;
            let back: IUnknown = apo.cast().map_err(|e| e.to_string())?;
            check(
                back.as_raw() == outer.as_raw(),
                "aggregated interfaces answer for the outer object",
            )?;
            drop(back);
            apo.Initialize(&[])
                .map_err(|e| format!("Initialize: {e}"))?;
            let fmt: IAudioMediaType = FloatStereo.into();
            apo.IsOutputFormatSupported(None, &fmt)
                .map_err(|e| format!("IsOutputFormatSupported: {e}"))?;
            let cfg: IAudioProcessingObjectConfiguration =
                outer.cast().map_err(|e| e.to_string())?;
            let desc = APO_CONNECTION_DESCRIPTOR {
                Type: APO_CONNECTION_BUFFER_TYPE_EXTERNAL,
                pBuffer: 0,
                u32MaxFrameCount: 480,
                pFormat: core::mem::ManuallyDrop::new(Some(fmt.clone())),
                u32Signature: 0,
            };
            let d: *const APO_CONNECTION_DESCRIPTOR = &desc;
            cfg.LockForProcess(&[d], &[d])
                .map_err(|e| format!("LockForProcess: {e}"))?;
            let rt: IAudioProcessingObjectRT = outer.cast().map_err(|e| e.to_string())?;
            let g = measure(&rt, 1000.0);
            println!("     1 kHz through the aggregated engine: {g:+.2} dB (expect +12)");
            check(
                (g - 12.0).abs() < 0.2,
                "EQ when aggregated, as audiodg creates it",
            )?;
            cfg.UnlockForProcess()
                .map_err(|e| format!("UnlockForProcess: {e}"))?;
            core::mem::ManuallyDrop::into_inner(desc.pFormat);
            drop((rt, cfg, apo));
            check(
                (*raw).refs.load(Ordering::SeqCst) == 1,
                "aggregated reference counts balance",
            )?;
            drop(outer); // frees the outer object and, with it, the engine
        }
        Ok(())
    }

    const CLSID_CHAIN_TEST_TEXT: &str = "{0FEBC01A-A301-491F-BBCD-17284A13CB62}";
    /// Where setup records the device's own effect (see chain.rs).
    const PKEY_CHAINED_EFFECT: PROPERTYKEY = PROPERTYKEY {
        fmtid: CLSID,
        pid: 1,
    };

    fn chain_test_key() -> String {
        format!(r"Software\Classes\CLSID\{CLSID_CHAIN_TEST_TEXT}")
    }

    /// The device's effect settings, as the audio engine hands them over:
    /// they name the engine's test class as the device's own effect.
    #[implement(IPropertyStore)]
    struct FxProperties;

    impl IPropertyStore_Impl for FxProperties_Impl {
        fn GetCount(&self) -> Result<u32> {
            Ok(1)
        }
        fn GetAt(&self, _i: u32, key: *mut PROPERTYKEY) -> Result<()> {
            unsafe { *key = PKEY_CHAINED_EFFECT };
            Ok(())
        }
        fn GetValue(&self, key: *const PROPERTYKEY) -> Result<PROPVARIANT> {
            let key = unsafe { *key };
            if key.fmtid != PKEY_CHAINED_EFFECT.fmtid || key.pid != PKEY_CHAINED_EFFECT.pid {
                return Ok(PROPVARIANT::default());
            }
            // The caller frees the string with CoTaskMemFree.
            let text: Vec<u16> = CLSID_CHAIN_TEST_TEXT.encode_utf16().chain([0]).collect();
            let p = unsafe { CoTaskMemAlloc(text.len() * 2) } as *mut u16;
            unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), p, text.len()) };
            Ok(PROPVARIANT {
                Anonymous: PROPVARIANT_0 {
                    Anonymous: core::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                        vt: VT_LPWSTR,
                        wReserved1: 0,
                        wReserved2: 0,
                        wReserved3: 0,
                        Anonymous: PROPVARIANT_0_0_0 { pwszVal: PWSTR(p) },
                    }),
                },
            })
        }
        fn SetValue(&self, _key: *const PROPERTYKEY, _v: *const PROPVARIANT) -> Result<()> {
            Ok(())
        }
        fn Commit(&self) -> Result<()> {
            Ok(())
        }
    }

    /// Registers the engine's test class, pointing at the DLL: for this
    /// user (Windows), and for the machine where allowed (Wine reads only
    /// that). At least one must work.
    unsafe fn register_chain_test(dll: HMODULE) -> std::result::Result<(), String> {
        let user = unsafe { register_chain_test_in(HKEY_CURRENT_USER, dll) };
        let machine = unsafe { register_chain_test_in(HKEY_LOCAL_MACHINE, dll) };
        user.or(machine)
    }

    unsafe fn register_chain_test_in(root: HKEY, dll: HMODULE) -> std::result::Result<(), String> {
        let mut path = [0u16; 1024];
        let n = unsafe { GetModuleFileNameW(Some(dll), &mut path) } as usize;
        let path = &path[..=n]; // with the terminating zero
        let both: Vec<u16> = "Both".encode_utf16().chain([0]).collect();
        let key = HSTRING::from(format!(r"{}\InprocServer32", chain_test_key()));
        let model = HSTRING::from("ThreadingModel");
        for (name, value) in [(PCWSTR::null(), path), (PCWSTR(model.as_ptr()), &both[..])] {
            unsafe {
                RegSetKeyValueW(
                    root,
                    &key,
                    name,
                    REG_SZ.0,
                    Some(value.as_ptr() as _),
                    (value.len() * 2) as u32,
                )
            }
            .ok()
            .map_err(|e| format!("register the test class: {e}"))?;
        }
        Ok(())
    }

    /// Runs the engine with its test class recorded as the device's own
    /// effect: it must create it and run the sound through both, so the
    /// +12 dB band comes out at +24 dB.
    unsafe fn chained(factory: &IClassFactory, dll: HMODULE) -> std::result::Result<(), String> {
        unsafe { register_chain_test(dll)? };
        let result = unsafe { chained_run(factory) };
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            let _ = unsafe { RegDeleteTreeW(root, &HSTRING::from(chain_test_key())) };
        }
        result
    }

    unsafe fn chained_run(factory: &IClassFactory) -> std::result::Result<(), String> {
        unsafe {
            let store: IPropertyStore = FxProperties.into();
            let init = APOInitSystemEffects2 {
                APOInit: APOInitBaseStruct {
                    cbSize: core::mem::size_of::<APOInitSystemEffects2>() as u32,
                    clsid: CLSID,
                },
                pAPOEndpointProperties: core::mem::ManuallyDrop::new(None),
                pAPOSystemEffectsProperties: core::mem::ManuallyDrop::new(Some(store)),
                pReserved: core::ptr::null_mut(),
                pDeviceCollection: core::mem::ManuallyDrop::new(None),
                nSoftwareIoDeviceInCollection: 0,
                nSoftwareIoConnectorIndex: 0,
                AudioProcessingMode: GUID::from_u128(0xc18e2f7e_933d_4965_b7d1_1eef228d2af3),
                InitializeForDiscoveryOnly: false.into(),
            };
            let bytes = core::slice::from_raw_parts(
                &init as *const _ as *const u8,
                core::mem::size_of::<APOInitSystemEffects2>(),
            );
            let apo: IAudioProcessingObject = factory
                .CreateInstance(None)
                .map_err(|e| format!("CreateInstance: {e}"))?;
            apo.Initialize(bytes)
                .map_err(|e| format!("Initialize with setup data: {e}"))?;
            let fmt: IAudioMediaType = FloatStereo.into();
            apo.IsInputFormatSupported(None, &fmt)
                .map_err(|e| format!("IsInputFormatSupported: {e}"))?;
            apo.IsOutputFormatSupported(None, &fmt)
                .map_err(|e| format!("IsOutputFormatSupported: {e}"))?;
            let cfg: IAudioProcessingObjectConfiguration = apo.cast().map_err(|e| e.to_string())?;
            let desc = APO_CONNECTION_DESCRIPTOR {
                Type: APO_CONNECTION_BUFFER_TYPE_EXTERNAL,
                pBuffer: 0,
                u32MaxFrameCount: 480,
                pFormat: core::mem::ManuallyDrop::new(Some(fmt.clone())),
                u32Signature: 0,
            };
            let d: *const APO_CONNECTION_DESCRIPTOR = &desc;
            cfg.LockForProcess(&[d], &[d])
                .map_err(|e| format!("LockForProcess: {e}"))?;
            let rt: IAudioProcessingObjectRT = apo.cast().map_err(|e| e.to_string())?;
            let g = measure(&rt, 1000.0);
            println!(
                "     1 kHz through the Equaliser and the chained effect: {g:+.2} dB (expect +24)"
            );
            let r = check(
                (g - 24.0).abs() < 0.3,
                "runs the device's own effect after the EQ",
            );
            cfg.UnlockForProcess()
                .map_err(|e| format!("UnlockForProcess: {e}"))?;
            core::mem::ManuallyDrop::into_inner(desc.pFormat);
            core::mem::ManuallyDrop::into_inner(init.pAPOSystemEffectsProperties);
            r
        }
    }

    type GetClassObject =
        unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut core::ffi::c_void) -> HRESULT;

    fn check(cond: bool, what: &str) -> std::result::Result<(), String> {
        if cond {
            println!("ok   {what}");
            Ok(())
        } else {
            Err(what.to_string())
        }
    }

    /// Gain in dB of a sine at `freq` through the APO, in 10 ms buffers.
    fn measure(rt: &IAudioProcessingObjectRT, freq: f64) -> f64 {
        let frames = 480;
        let mut phase = 0.0f64;
        let (mut sum_in, mut sum_out, mut n) = (0.0f64, 0.0f64, 0usize);
        for block in 0..100 {
            let mut buf: Vec<f32> = (0..frames)
                .flat_map(|_| {
                    phase += 2.0 * PI * freq / RATE as f64;
                    let v = (phase.sin() * 0.1) as f32;
                    [v; CH as usize]
                })
                .collect();
            let input: Vec<f32> = buf.clone();
            let mut conn = APO_CONNECTION_PROPERTY {
                pBuffer: buf.as_mut_ptr() as usize,
                u32ValidFrameCount: frames,
                u32BufferFlags: BUFFER_VALID,
                u32Signature: 0,
            };
            let in_ptr: *const APO_CONNECTION_PROPERTY = &conn;
            let mut out_ptr: *mut APO_CONNECTION_PROPERTY = &mut conn;
            unsafe { rt.APOProcess(1, &in_ptr, 1, &mut out_ptr) };
            if block >= 50 {
                sum_in += input.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
                sum_out += buf.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
                n += 1;
            }
        }
        let _ = n;
        10.0 * (sum_out / sum_in).log10()
    }

    pub fn run() -> std::result::Result<(), String> {
        // Settings first: +12 dB at band 6 (1 kHz bell). With `--from-app`,
        // The Equaliser itself must already have written that (band 6 at +12 dB),
        // which checks the whole chain from the panel to the sound.
        let path = engine_settings::default_path();
        let from_app = std::env::args().any(|a| a == "--from-app");
        if !from_app {
            let mut s = EqSettings::default();
            s.bands[5].gain_db = 12.0;
            engine_settings::write(&path, &s)
                .map_err(|e| format!("write {}: {e}", path.display()))?;
        }
        println!("settings file {}", path.display());

        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let dll = LoadLibraryW(w!("taw_equaliser_apo.dll"))
                .map_err(|e| format!("LoadLibrary: {e}"))?;
            let proc = GetProcAddress(dll, PCSTR(c"DllGetClassObject".as_ptr() as _))
                .ok_or("no DllGetClassObject")?;
            let get: GetClassObject = core::mem::transmute(proc);
            let mut factory: Option<IClassFactory> = None;
            get(&CLSID, &IClassFactory::IID, &mut factory as *mut _ as _)
                .ok()
                .map_err(|e| format!("DllGetClassObject: {e}"))?;
            let factory = factory.ok_or("no factory")?;
            aggregated(&factory)?;
            chained(&factory, dll)?;
            let apo: IAudioProcessingObject = factory
                .CreateInstance(None)
                .map_err(|e| format!("CreateInstance: {e}"))?;
            check(true, "created the engine through COM")?;

            let props = apo.GetRegistrationProperties().map_err(|e| e.to_string())?;
            let name = String::from_utf16_lossy(&(*props).szFriendlyName)
                .trim_end_matches('\0')
                .to_string();
            let flags = (*props).Flags.0;
            CoTaskMemFree(Some(props as _));
            check(
                name == "TAW Equaliser" && flags & APO_FLAG_INPLACE.0 != 0,
                "registration properties",
            )?;

            apo.Initialize(&[])
                .map_err(|e| format!("Initialize: {e}"))?;
            check(apo.Initialize(&[]).is_err(), "second Initialize is refused")?;

            let fmt: IAudioMediaType = FloatStereo.into();
            let ok = apo
                .IsInputFormatSupported(None, &fmt)
                .map_err(|e| format!("IsInputFormatSupported: {e}"))?;
            check(ok.as_raw() == fmt.as_raw(), "accepts 32-bit float")?;

            let cfg: IAudioProcessingObjectConfiguration = apo.cast().map_err(|e| e.to_string())?;
            let desc = APO_CONNECTION_DESCRIPTOR {
                Type: APO_CONNECTION_BUFFER_TYPE_EXTERNAL,
                pBuffer: 0,
                u32MaxFrameCount: 480,
                pFormat: core::mem::ManuallyDrop::new(Some(fmt.clone())),
                u32Signature: 0,
            };
            let d: *const APO_CONNECTION_DESCRIPTOR = &desc;
            cfg.LockForProcess(&[d], &[d])
                .map_err(|e| format!("LockForProcess: {e}"))?;
            check(
                apo.GetInputChannelCount().map_err(|e| e.to_string())? == CH,
                "locked for 2 channels",
            )?;

            let rt: IAudioProcessingObjectRT = apo.cast().map_err(|e| e.to_string())?;
            let g = measure(&rt, 1000.0);
            println!("     1 kHz through the engine: {g:+.2} dB (expect +12)");
            check((g - 12.0).abs() < 0.2, "EQ from the settings file")?;
            let g5 = measure(&rt, 5000.0);
            println!("     5 kHz: {g5:+.2} dB (expect about +0.6)");
            check((g5 - 0.6).abs() < 0.2, "the bell's shape")?;

            if from_app {
                return Ok(());
            }

            // Live change: flat, picked up by the watcher.
            engine_settings::write(&path, &EqSettings::default()).map_err(|e| e.to_string())?;
            std::thread::sleep(std::time::Duration::from_millis(400));
            let g = measure(&rt, 1000.0);
            println!("     1 kHz after going flat: {g:+.2} dB (expect 0)");
            check(g.abs() < 0.1, "picks up settings changes live")?;

            // The cut filters: high-pass, low-pass and notch.
            for (kind, freq, probe, expect) in [
                (eq_core::dsp::FilterKind::HighPass, 1000.0, 125.0, -30.0),
                (eq_core::dsp::FilterKind::LowPass, 1000.0, 8000.0, -30.0),
                (eq_core::dsp::FilterKind::Notch, 1000.0, 1000.0, -40.0),
            ] {
                let mut s = EqSettings::default();
                s.bands[5].kind = kind;
                s.bands[5].freq = freq;
                s.bands[5].q = 0.707;
                engine_settings::write(&path, &s).map_err(|e| e.to_string())?;
                std::thread::sleep(std::time::Duration::from_millis(400));
                let g = measure(&rt, probe);
                println!("     {kind:?} at 1 kHz, {probe} Hz: {g:+.1} dB (expect below {expect})");
                check(g < expect, &format!("{kind:?} works in the engine"))?;
            }
            engine_settings::write(&path, &EqSettings::default()).map_err(|e| e.to_string())?;
            std::thread::sleep(std::time::Duration::from_millis(400));

            // Silent buffers pass straight through.
            let mut buf = vec![0.5f32; 960];
            let mut conn = APO_CONNECTION_PROPERTY {
                pBuffer: buf.as_mut_ptr() as usize,
                u32ValidFrameCount: 480,
                u32BufferFlags: BUFFER_SILENT,
                u32Signature: 0,
            };
            let in_ptr: *const APO_CONNECTION_PROPERTY = &conn;
            let mut out_ptr: *mut APO_CONNECTION_PROPERTY = &mut conn;
            rt.APOProcess(1, &in_ptr, 1, &mut out_ptr);
            check(
                conn.u32BufferFlags == BUFFER_SILENT && buf[0] == 0.5,
                "leaves silent buffers alone",
            )?;

            // The engine reports what it's doing once a second.
            std::thread::sleep(std::time::Duration::from_millis(1300));
            let status = path.with_file_name("engine-status.txt");
            let text = std::fs::read_to_string(&status).unwrap_or_default();
            for line in text.lines() {
                println!("     | {line}");
            }
            check(
                text.contains("streams running: 1") && text.contains("48000 Hz, 2 ch"),
                "writes its status report",
            )?;

            cfg.UnlockForProcess()
                .map_err(|e| format!("UnlockForProcess: {e}"))?;
            check(cfg.UnlockForProcess().is_err(), "second Unlock is refused")?;
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(status);
        }
        Ok(())
    }
}
