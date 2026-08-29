//! Minimal safe wrapper over Carbon's global-hotkey API (`RegisterEventHotKey`).
//!
//! Hand-written FFI: the only crates.io wrapper (`carbonhotkey`) compiles a Swift
//! bridge that requires a Swift toolchain / Xcode, which isn't available with
//! Command Line Tools alone. Per ARCHITECTURE.md, Carbon `RegisterEventHotKey`
//! is the default hotkey backend (no Accessibility permission required).
//! Only the Carbon *system framework* is linked.
//!
//! Hotkey ids: 1 is the Option+Space toggle, ids 2+ are the configured
//! global target shortcuts (in config order, see ADR 0002).

#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    missing_docs
)]

use core::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::core::item::Target;

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
// kEventParamDirectObject, '----' (CarbonEventsCore.h)
const k_event_param_direct_object: OSType = 0x2d2d_2d2d;
// typeEventHotKeyID, 'hkid' (CarbonEvents.h)
const type_event_hot_key_id: OSType = 0x686b_6964;

// Option+Space
const k_vk_space: u32 = 0x31;
const option_key: u32 = 1 << 11; // 0x0800
// Carbon modifier bits for user-configured combos.
const cmd_key: u32 = 1 << 8; // 0x0100
const shift_key: u32 = 1 << 9; // 0x0200
const control_key: u32 = 1 << 12; // 0x1000

const HOTKEY_SIGNATURE: OSType = 0x41_45_52_46; // 'AERF'
const HOTKEY_ID: u32 = 1; // Option+Space toggle
const GLOBAL_BASE_ID: u32 = 2; // configured global target shortcuts

/// A global shortcut bound to a target (parsed combo + the target to run).
#[derive(Clone)]
pub struct GlobalBinding {
    /// Carbon virtual keycode of the key.
    pub keycode: u32,
    /// Carbon modifier mask (cmd/shift/opt/ctrl bits).
    pub modifiers: u32,
    /// The target to run when the combo fires.
    pub target: Target,
    /// The configured combo, for diagnostics.
    pub label: String,
}

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

    fn GetEventParameter(
        in_event: EventRef,
        in_name: OSType,
        in_desired_type: OSType,
        out_actual_type: *mut OSType,
        in_data_size: u64,
        out_actual_size: *mut u64,
        out_data: *mut c_void,
    ) -> OSStatus;

}

// Keep the Carbon references alive for the process lifetime (never freed on purpose).
static HANDLER_REF: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// The `Vec<EventHotKeyRef>` itself, kept alive by leaking it (raw pointers
/// are not `Sync`, so it cannot sit in a `OnceLock` directly).
static GLOBAL_HOTKEY_REFS: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// The targets behind the global hotkeys, indexed by `id - GLOBAL_BASE_ID`.
static GLOBAL_TARGETS: OnceLock<Vec<Target>> = OnceLock::new();

unsafe extern "C" fn hotkey_handler(
    _handler_call_ref: *mut c_void,
    in_event: EventRef,
    _user_data: *mut c_void,
) -> OSStatus {
    let mut id = EventHotKeyID {
        signature: 0,
        id: 0,
    };
    let status = unsafe {
        GetEventParameter(
            in_event,
            k_event_param_direct_object,
            type_event_hot_key_id,
            core::ptr::null_mut(),
            core::mem::size_of::<EventHotKeyID>() as u64,
            core::ptr::null_mut(),
            core::ptr::addr_of_mut!(id).cast(),
        )
    };
    if status == noErr && id.signature == HOTKEY_SIGNATURE {
        if id.id == HOTKEY_ID {
            crate::ui::window::toggle();
        } else if let Some(targets) = GLOBAL_TARGETS.get()
            && let Some(target) = targets.get((id.id - GLOBAL_BASE_ID) as usize)
        {
            crate::core::executor::execute(target);
        }
    }
    noErr
}

/// Parse a shortcut combo like "opt+d" or "cmd+shift+r" into a Carbon
/// virtual keycode and modifier mask. Returns `None` for unknown keys,
/// missing keys, or combos without at least one of cmd/ctrl/opt (shift
/// alone is not a valid global hotkey).
pub fn parse_combo(combo: &str) -> Option<(u32, u32)> {
    let mut modifiers: u32 = 0;
    let mut key: Option<String> = None;
    for token in combo.split('+') {
        let token = token.trim().to_ascii_lowercase();
        match token.as_str() {
            "cmd" | "command" | "super" => modifiers |= cmd_key,
            "ctrl" | "control" => modifiers |= control_key,
            "alt" | "option" | "opt" => modifiers |= option_key,
            "shift" => modifiers |= shift_key,
            other if !other.is_empty() => key = Some(other.to_string()),
            _ => {}
        }
    }
    let key = key?;
    if modifiers & (cmd_key | control_key | option_key) == 0 {
        return None;
    }
    Some((keycode_for(&key)?, modifiers))
}

/// Carbon virtual keycode for a key name (values from Carbon `Events.h`).
fn keycode_for(key: &str) -> Option<u32> {
    const TABLE: [(&str, u32); 57] = [
        ("a", 0x00),
        ("s", 0x01),
        ("d", 0x02),
        ("f", 0x03),
        ("h", 0x04),
        ("g", 0x05),
        ("z", 0x06),
        ("x", 0x07),
        ("c", 0x08),
        ("v", 0x09),
        ("b", 0x0B),
        ("q", 0x0C),
        ("w", 0x0D),
        ("e", 0x0E),
        ("r", 0x0F),
        ("y", 0x10),
        ("t", 0x11),
        ("1", 0x12),
        ("2", 0x13),
        ("3", 0x14),
        ("4", 0x15),
        ("6", 0x16),
        ("5", 0x17),
        ("9", 0x19),
        ("7", 0x1A),
        ("8", 0x1C),
        ("0", 0x1D),
        ("o", 0x1F),
        ("u", 0x20),
        ("i", 0x22),
        ("p", 0x23),
        ("l", 0x25),
        ("j", 0x26),
        ("k", 0x28),
        ("n", 0x2D),
        ("m", 0x2E),
        ("return", 0x24),
        ("tab", 0x30),
        ("space", 0x31),
        ("backspace", 0x33),
        ("escape", 0x35),
        ("f1", 0x7A),
        ("f2", 0x78),
        ("f3", 0x63),
        ("f4", 0x76),
        ("f5", 0x60),
        ("f6", 0x61),
        ("f7", 0x62),
        ("f8", 0x64),
        ("f9", 0x65),
        ("f10", 0x6D),
        ("f11", 0x67),
        ("f12", 0x6F),
        ("left", 0x7B),
        ("right", 0x7C),
        ("up", 0x7E),
        ("down", 0x7D),
    ];
    TABLE
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, code)| *code)
}

/// Register the global Option+Space toggle plus the given global target
/// shortcuts, and install their event handler. Must be called on the main
/// thread. A conflicting global combo is skipped with a warning; a failed
/// toggle registration is an error.
pub fn install(globals: Vec<GlobalBinding>) -> Result<(), String> {
    if HANDLER_REF.load(Ordering::SeqCst).is_null() {
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
    }

    // Index order matches hotkey ids (GLOBAL_BASE_ID + i), including combos
    // that end up unregistered: their ids simply never fire.
    GLOBAL_TARGETS
        .set(globals.iter().map(|g| g.target.clone()).collect())
        .ok();

    let mut refs = Vec::new();
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
    refs.push(hotkey_ref);

    for (i, binding) in globals.iter().enumerate() {
        let mut ref_: EventHotKeyRef = core::ptr::null_mut();
        let status = unsafe {
            RegisterEventHotKey(
                binding.keycode,
                binding.modifiers,
                EventHotKeyID {
                    signature: HOTKEY_SIGNATURE,
                    id: GLOBAL_BASE_ID + i as u32,
                },
                GetApplicationEventTarget(),
                0,
                &mut ref_,
            )
        };
        if status != noErr {
            eprintln!(
                "aerofi: warning: could not register global shortcut {}: OSStatus {status} (is the combo taken?)",
                binding.label
            );
            continue;
        }
        refs.push(ref_);
    }

    // Leak the ref list: hotkeys live for the process lifetime (never freed
    // on purpose, same as the single-toggle pattern above).
    GLOBAL_HOTKEY_REFS.store(
        Box::into_raw(Box::new(refs)) as *mut c_void,
        Ordering::SeqCst,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_combo_requires_a_strong_modifier() {
        assert!(parse_combo("shift+r").is_none());
        assert!(parse_combo("r").is_none());
        assert!(parse_combo("cmd+").is_none());
        assert!(parse_combo("").is_none());
    }

    #[test]
    fn parse_combo_maps_modifiers_and_keys() {
        let (code, mods) = parse_combo("opt+d").expect("opt+d");
        assert_eq!(code, 0x02);
        assert_eq!(mods, option_key);

        let (code, mods) = parse_combo("cmd+shift+r").expect("cmd+shift+r");
        assert_eq!(code, 0x0F);
        assert_eq!(mods, cmd_key | shift_key);

        let (code, mods) = parse_combo("ctrl+alt+f12").expect("ctrl+alt+f12");
        assert_eq!(code, 0x6F);
        assert_eq!(mods, control_key | option_key);

        assert!(parse_combo("cmd+qwert").is_none());
    }
}
