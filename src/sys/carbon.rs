//! Minimal safe wrapper over Carbon's global-hotkey API (`RegisterEventHotKey`).
//!
//! Hand-written FFI: the only crates.io wrapper (`carbonhotkey`) compiles a Swift
//! bridge that requires a Swift toolchain / Xcode, which isn't available with
//! Command Line Tools alone. Per ARCHITECTURE.md, Carbon `RegisterEventHotKey`
//! is the default hotkey backend (no Accessibility permission required).
//! Only the Carbon *system framework* is linked.

#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    missing_docs
)]

use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

type OSStatus = i32;
type OSType = u32;
type ItemCount = u64;
type OptionBits = u32;

type EventTargetRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventRef = *mut c_void;
type EventHotKeyRef = *mut c_void;

const noErr: OSStatus = 0;

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: OSType,
    id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventTypeSpec {
    event_class: OSType,
    event_kind: u32,
}

// 'keyb'
const k_event_class_keyboard: OSType = 0x6b65_7962;
// kEventHotKeyPressed
const k_event_hot_key_pressed: u32 = 5;

// Option+Space
const k_vk_space: u32 = 0x31;
const option_key: u32 = 1 << 11; // 0x0800

const HOTKEY_SIGNATURE: OSType = 0x41_45_52_46; // 'AERF'
const HOTKEY_ID: u32 = 1;

type EventHandlerProcPtr = unsafe extern "C" fn(
    in_handler_call_ref: *mut c_void,
    in_event: EventRef,
    in_user_data: *mut c_void,
) -> OSStatus;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn RegisterEventHotKey(
        in_hot_key_code: u32,
        in_hot_key_modifiers: u32,
        in_hot_key_id: EventHotKeyID,
        in_target: EventTargetRef,
        in_options: OptionBits,
        out_ref: *mut EventHotKeyRef,
    ) -> OSStatus;

    fn GetApplicationEventTarget() -> EventTargetRef;

    fn InstallEventHandler(
        in_target: EventTargetRef,
        in_handler: EventHandlerProcPtr,
        in_num_types: ItemCount,
        in_list: *const EventTypeSpec,
        in_user_data: *mut c_void,
        out_ref: *mut EventHandlerRef,
    ) -> OSStatus;
}

// Keep the Carbon references alive for the process lifetime (never freed on purpose).
static HANDLER_REF: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static HOTKEY_REF: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

unsafe extern "C" fn hotkey_handler(
    _handler_call_ref: *mut c_void,
    _event: EventRef,
    _user_data: *mut c_void,
) -> OSStatus {
    // We register exactly one hotkey on the application event target, so any
    // hotkey event that reaches this handler is ours.
    crate::ui::window::toggle();
    noErr
}

/// Register the global Option+Space hotkey and install its event handler.
/// Must be called on the main thread.
pub fn install() -> Result<(), String> {
    let event_types = [EventTypeSpec {
        event_class: k_event_class_keyboard,
        event_kind: k_event_hot_key_pressed,
    }];
    let mut handler_ref: EventHandlerRef = core::ptr::null_mut();
    let status = unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            hotkey_handler,
            event_types.len() as ItemCount,
            event_types.as_ptr(),
            core::ptr::null_mut(),
            &mut handler_ref,
        )
    };
    if status != noErr {
        return Err(format!("InstallEventHandler failed with OSStatus {status}"));
    }
    HANDLER_REF.store(handler_ref, Ordering::SeqCst);

    let mut hotkey_ref: EventHotKeyRef = core::ptr::null_mut();
    let status = unsafe {
        RegisterEventHotKey(
            k_vk_space,
            option_key,
            EventHotKeyID {
                signature: HOTKEY_SIGNATURE,
                id: HOTKEY_ID,
            },
            GetApplicationEventTarget(),
            0,
            &mut hotkey_ref,
        )
    };
    if status != noErr {
        return Err(format!(
            "RegisterEventHotKey failed with OSStatus {status} (is Option+Space already taken?)"
        ));
    }
    HOTKEY_REF.store(hotkey_ref, Ordering::SeqCst);
    Ok(())
}
