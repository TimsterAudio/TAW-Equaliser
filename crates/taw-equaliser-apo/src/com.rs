//! The COM object Windows sees, built by hand around `apo::Engine`.
//!
//! The Windows audio engine creates effects through COM *aggregation*: it
//! passes its own "outer" object to `CreateInstance`, asks for our private
//! (non-delegating) `IUnknown`, and from then on every `QueryInterface`,
//! `AddRef` and `Release` on our interfaces goes to that outer object. The
//! `windows` crate's `#[implement]` can't be aggregated, so the object is
//! laid out here directly: one vtable pointer per interface, then the
//! reference count, the outer object and the engine.
//!
//! Created on its own (outer is null), the object is its own outer object,
//! so the same code serves both cases.

use crate::apo::Engine;
use core::ffi::c_void;
use core::mem::offset_of;
use std::sync::atomic::{fence, AtomicU32, Ordering};
use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_NOINTERFACE, E_POINTER, HANDLE, S_OK};
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows_core::{
    implement, IUnknown, IUnknown_Vtbl, Interface, Ref, Result, BOOL, GUID, HRESULT,
};

#[repr(C)]
struct ApoObject {
    /// The private IUnknown: owns the reference count. Must come first.
    nondelegating: *const IUnknown_Vtbl,
    apo: *const IAudioProcessingObject_Vtbl,
    rt: *const IAudioProcessingObjectRT_Vtbl,
    config: *const IAudioProcessingObjectConfiguration_Vtbl,
    effects: *const IAudioSystemEffects2_Vtbl,
    refs: AtomicU32,
    /// Where the public IUnknown calls go: the audio engine's object, or
    /// this object's own `nondelegating` when it isn't aggregated. Not
    /// reference counted (the outer object owns us, not the other way).
    outer: *mut c_void,
    engine: Engine,
}

const APO: usize = offset_of!(ApoObject, apo);
const RT: usize = offset_of!(ApoObject, rt);
const CONFIG: usize = offset_of!(ApoObject, config);
const EFFECTS: usize = offset_of!(ApoObject, effects);

/// The object behind an interface pointer whose vtable sits at `OFFSET`.
unsafe fn object<'a, const OFFSET: usize>(this: *mut c_void) -> &'a ApoObject {
    unsafe { &*((this as *const u8).sub(OFFSET) as *const ApoObject) }
}

unsafe fn outer_vtbl(o: &ApoObject) -> &IUnknown_Vtbl {
    unsafe { &**(o.outer as *const *const IUnknown_Vtbl) }
}

// ------------------------------------------------- non-delegating IUnknown

unsafe extern "system" fn nd_query(
    this: *mut c_void,
    iid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    unsafe { *ppv = core::ptr::null_mut() };
    if iid.is_null() {
        return E_POINTER;
    }
    let o = unsafe { object::<0>(this) };
    let iid = unsafe { *iid };
    let field: *const c_void = if iid == IUnknown::IID {
        // Only the private IUnknown is counted here; the rest count
        // through the outer object.
        unsafe { nd_add_ref(this) };
        unsafe { *ppv = this };
        return S_OK;
    } else if iid == IAudioProcessingObject::IID {
        &o.apo as *const _ as _
    } else if iid == IAudioProcessingObjectRT::IID {
        &o.rt as *const _ as _
    } else if iid == IAudioProcessingObjectConfiguration::IID {
        &o.config as *const _ as _
    } else if iid == IAudioSystemEffects::IID || iid == IAudioSystemEffects2::IID {
        &o.effects as *const _ as _
    } else {
        return E_NOINTERFACE;
    };
    unsafe {
        (outer_vtbl(o).AddRef)(o.outer);
        *ppv = field as *mut c_void;
    }
    S_OK
}

unsafe extern "system" fn nd_add_ref(this: *mut c_void) -> u32 {
    unsafe { object::<0>(this) }
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

unsafe extern "system" fn nd_release(this: *mut c_void) -> u32 {
    let left = unsafe { object::<0>(this) }
        .refs
        .fetch_sub(1, Ordering::Release)
        - 1;
    if left == 0 {
        fence(Ordering::Acquire);
        drop(unsafe { Box::from_raw(this as *mut ApoObject) });
    }
    left
}

static NONDELEGATING: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: nd_query,
    AddRef: nd_add_ref,
    Release: nd_release,
};

// ------------------------------------- delegating IUnknown (every interface)

unsafe extern "system" fn d_query<const OFFSET: usize>(
    this: *mut c_void,
    iid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    let o = unsafe { object::<OFFSET>(this) };
    unsafe { (outer_vtbl(o).QueryInterface)(o.outer, iid, ppv) }
}

unsafe extern "system" fn d_add_ref<const OFFSET: usize>(this: *mut c_void) -> u32 {
    let o = unsafe { object::<OFFSET>(this) };
    unsafe { (outer_vtbl(o).AddRef)(o.outer) }
}

unsafe extern "system" fn d_release<const OFFSET: usize>(this: *mut c_void) -> u32 {
    let o = unsafe { object::<OFFSET>(this) };
    unsafe { (outer_vtbl(o).Release)(o.outer) }
}

const fn delegating<const OFFSET: usize>() -> IUnknown_Vtbl {
    IUnknown_Vtbl {
        QueryInterface: d_query::<OFFSET>,
        AddRef: d_add_ref::<OFFSET>,
        Release: d_release::<OFFSET>,
    }
}

fn hresult(r: Result<()>) -> HRESULT {
    r.into()
}

// ---------------------------------------------------- IAudioProcessingObject

unsafe extern "system" fn reset(this: *mut c_void) -> HRESULT {
    hresult(unsafe { object::<APO>(this) }.engine.reset())
}

unsafe extern "system" fn get_latency(this: *mut c_void, latency: *mut i64) -> HRESULT {
    if latency.is_null() {
        return E_POINTER;
    }
    unsafe { *latency = object::<APO>(this).engine.latency() };
    S_OK
}

unsafe extern "system" fn get_registration_properties(
    this: *mut c_void,
    out: *mut *mut APO_REG_PROPERTIES,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    match unsafe { object::<APO>(this) }
        .engine
        .registration_properties()
    {
        Ok(p) => {
            unsafe { *out = p };
            S_OK
        }
        Err(e) => e.code(),
    }
}

unsafe extern "system" fn initialize(this: *mut c_void, size: u32, data: *const u8) -> HRESULT {
    hresult(unsafe { object::<APO>(this) }.engine.initialize(size, data))
}

unsafe fn format_supported(
    engine: &Engine,
    input: bool,
    opposite: *mut c_void,
    requested: *mut c_void,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { *out = core::ptr::null_mut() };
    let opposite = unsafe { IAudioMediaType::from_raw_borrowed(&opposite) };
    let requested = unsafe { IAudioMediaType::from_raw_borrowed(&requested) };
    match engine.check_format(opposite, requested, input) {
        Ok(t) => {
            unsafe { *out = t.into_raw() };
            S_OK
        }
        Err(e) => e.code(),
    }
}

unsafe extern "system" fn is_input_format_supported(
    this: *mut c_void,
    opposite: *mut c_void,
    requested: *mut c_void,
    out: *mut *mut c_void,
) -> HRESULT {
    let engine = unsafe { &object::<APO>(this).engine };
    unsafe { format_supported(engine, true, opposite, requested, out) }
}

unsafe extern "system" fn is_output_format_supported(
    this: *mut c_void,
    opposite: *mut c_void,
    requested: *mut c_void,
    out: *mut *mut c_void,
) -> HRESULT {
    let engine = unsafe { &object::<APO>(this).engine };
    unsafe { format_supported(engine, false, opposite, requested, out) }
}

unsafe extern "system" fn get_input_channel_count(this: *mut c_void, out: *mut u32) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    match unsafe { object::<APO>(this) }.engine.input_channel_count() {
        Ok(n) => {
            unsafe { *out = n };
            S_OK
        }
        Err(e) => e.code(),
    }
}

static APO_VTBL: IAudioProcessingObject_Vtbl = IAudioProcessingObject_Vtbl {
    base__: delegating::<APO>(),
    Reset: reset,
    GetLatency: get_latency,
    GetRegistrationProperties: get_registration_properties,
    Initialize: initialize,
    IsInputFormatSupported: is_input_format_supported,
    IsOutputFormatSupported: is_output_format_supported,
    GetInputChannelCount: get_input_channel_count,
};

// -------------------------------------------------- IAudioProcessingObjectRT

unsafe extern "system" fn apo_process(
    this: *mut c_void,
    n_in: u32,
    inputs: *const *const APO_CONNECTION_PROPERTY,
    n_out: u32,
    outputs: *mut *mut APO_CONNECTION_PROPERTY,
) {
    unsafe { object::<RT>(this) }
        .engine
        .process(n_in, inputs, n_out, outputs)
}

unsafe extern "system" fn calc_frames(_this: *mut c_void, frames: u32) -> u32 {
    frames
}

static RT_VTBL: IAudioProcessingObjectRT_Vtbl = IAudioProcessingObjectRT_Vtbl {
    base__: delegating::<RT>(),
    APOProcess: apo_process,
    CalcInputFrames: calc_frames,
    CalcOutputFrames: calc_frames,
};

// ---------------------------------------- IAudioProcessingObjectConfiguration

unsafe extern "system" fn lock_for_process(
    this: *mut c_void,
    n_in: u32,
    inputs: *const *const APO_CONNECTION_DESCRIPTOR,
    n_out: u32,
    outputs: *const *const APO_CONNECTION_DESCRIPTOR,
) -> HRESULT {
    hresult(
        unsafe { object::<CONFIG>(this) }
            .engine
            .lock_for_process(n_in, inputs, n_out, outputs),
    )
}

unsafe extern "system" fn unlock_for_process(this: *mut c_void) -> HRESULT {
    hresult(
        unsafe { object::<CONFIG>(this) }
            .engine
            .unlock_for_process(),
    )
}

static CONFIG_VTBL: IAudioProcessingObjectConfiguration_Vtbl =
    IAudioProcessingObjectConfiguration_Vtbl {
        base__: delegating::<CONFIG>(),
        LockForProcess: lock_for_process,
        UnlockForProcess: unlock_for_process,
    };

// ------------------------------------------------------ IAudioSystemEffects2

unsafe extern "system" fn get_effects_list(
    this: *mut c_void,
    ids: *mut *mut GUID,
    count: *mut u32,
    event: HANDLE,
) -> HRESULT {
    hresult(
        unsafe { object::<EFFECTS>(this) }
            .engine
            .effects_list(ids, count, event),
    )
}

static EFFECTS_VTBL: IAudioSystemEffects2_Vtbl = IAudioSystemEffects2_Vtbl {
    base__: IAudioSystemEffects_Vtbl {
        base__: delegating::<EFFECTS>(),
    },
    GetEffectsList: get_effects_list,
};

// ------------------------------------------------------------------ factory

#[implement(IClassFactory)]
pub struct Factory {
    /// Whether engines it makes run the device's own effect after the EQ
    /// (off only for the harness's test class, so it can't chain itself).
    pub chains: bool,
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> Result<()> {
        if ppv.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = core::ptr::null_mut() };
        let outer = outer.as_ref().map_or(core::ptr::null_mut(), |u| u.as_raw());
        let aggregated = !outer.is_null();
        // COM's rule: an aggregated object hands out only its private
        // IUnknown at creation.
        if aggregated && unsafe { *riid } != IUnknown::IID {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let obj = Box::into_raw(Box::new(ApoObject {
            nondelegating: &NONDELEGATING,
            apo: &APO_VTBL,
            rt: &RT_VTBL,
            config: &CONFIG_VTBL,
            effects: &EFFECTS_VTBL,
            refs: AtomicU32::new(1),
            outer: core::ptr::null_mut(),
            engine: Engine::new(aggregated, self.chains),
        }));
        unsafe {
            (*obj).outer = if aggregated {
                outer
            } else {
                obj as *mut c_void
            };
            // The query takes its own reference; dropping the creation one
            // frees the object if the query failed.
            let hr = nd_query(obj as *mut c_void, riid, ppv);
            nd_release(obj as *mut c_void);
            hr.ok()
        }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}
