//! Narrow, private, UI-thread-only wrapper for the platform widget adapter.
use std::{
    ffi::{CString, c_char, c_void},
    path::{Path, PathBuf},
};
use synarith_viewer::Preview;

#[repr(i32)]
#[derive(Clone, Copy)]
pub enum Field {
    Source,
    Width,
    Height,
    Time,
    Samples,
    Backend,
    Output,
    Parameters,
    Format,
    Quality,
    Seed,
}
unsafe extern "C" {
    fn syar_ui_run(
        callback: unsafe extern "C" fn(*mut c_void, i32),
        context: *mut c_void,
        source: *const c_char,
    ) -> i32;
    fn syar_ui_get(field: i32, buffer: *mut c_char, capacity: usize) -> usize;
    fn syar_ui_set(field: i32, value: *const c_char);
    fn syar_ui_status(message: *const c_char, busy: bool);
    fn syar_ui_log(message: *const c_char);
    fn syar_ui_image(rgba: *const u8, width: i32, height: i32);
    fn syar_ui_choose(
        save: bool,
        suggested: *const c_char,
        extension: *const c_char,
        buffer: *mut c_char,
        capacity: usize,
    ) -> bool;
    fn syar_ui_confirm(message: *const c_char) -> bool;
    fn syar_ui_quit();
    fn syar_ui_capture(rgba: *mut u8, capacity: usize, width: *mut i32, height: *mut i32) -> usize;
}
fn string(text: &str) -> CString {
    CString::new(text.replace('\0', "")).expect("NUL removed")
}
pub struct Ui;
impl Ui {
    /// Caller keeps the callback context alive and uses this adapter on one UI thread.
    pub unsafe fn run(
        callback: unsafe extern "C" fn(*mut c_void, i32),
        context: *mut c_void,
        source: &str,
    ) -> i32 {
        // SAFETY: documented caller contract; source is borrowed for this call only.
        unsafe { syar_ui_run(callback, context, string(source).as_ptr()) }
    }
    pub fn get(field: Field) -> String {
        // SAFETY: null buffer with zero capacity queries the required UTF-8 length.
        let size =
            unsafe { syar_ui_get(field as i32, std::ptr::null_mut(), 0) }.clamp(1, 1_048_576);
        let mut buffer = vec![0u8; size];
        // SAFETY: adapter respects capacity; no event dispatch occurs inside getters.
        unsafe {
            syar_ui_get(field as i32, buffer.as_mut_ptr().cast(), size);
        }
        let length = buffer.iter().position(|b| *b == 0).unwrap_or(size);
        String::from_utf8_lossy(&buffer[..length]).into_owned()
    }
    pub fn set(field: Field, value: &str) {
        // SAFETY: temporary string remains alive for this synchronous call.
        unsafe {
            syar_ui_set(field as i32, string(value).as_ptr());
        }
    }
    pub fn status(message: &str, busy: bool) {
        // SAFETY: adapter copies the borrowed string.
        unsafe {
            syar_ui_status(string(message).as_ptr(), busy);
        }
    }
    pub fn log(message: &str) {
        // SAFETY: adapter copies the borrowed string.
        unsafe {
            syar_ui_log(string(message).as_ptr());
        }
    }
    pub fn image(image: &Preview) {
        // SAFETY: bounded decoded RGBA8 storage has width*height*4 bytes and is copied.
        unsafe {
            syar_ui_image(image.rgba.as_ptr(), image.width as i32, image.height as i32);
        }
    }
    pub fn choose(save: bool, suggested: &Path, extension: &str) -> Option<PathBuf> {
        let mut buffer = vec![0u8; 131_072];
        // SAFETY: adapter bounds output and borrows input strings only during the modal call.
        let accepted = unsafe {
            syar_ui_choose(
                save,
                string(&suggested.to_string_lossy()).as_ptr(),
                string(extension).as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        accepted.then(|| {
            let length = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
            PathBuf::from(String::from_utf8_lossy(&buffer[..length]).into_owned())
        })
    }
    pub fn confirm(message: &str) -> bool {
        // SAFETY: adapter borrows the string for its modal call only.
        unsafe { syar_ui_confirm(string(message).as_ptr()) }
    }
    pub fn quit() {
        // SAFETY: called on the active native event-loop thread.
        unsafe {
            syar_ui_quit();
        }
    }
    pub fn snapshot(path: &Path) -> Result<(), String> {
        let (mut width, mut height) = (0, 0);
        // SAFETY: null buffer requests size; dimension pointers refer to live stack values.
        let size = unsafe { syar_ui_capture(std::ptr::null_mut(), 0, &mut width, &mut height) };
        if size == 0 || size > 128 * 1024 * 1024 {
            return Err("Cannot capture native window.".into());
        }
        let mut bytes = vec![0u8; size];
        // SAFETY: adapter bounds the copy and returns zero if the window grew beyond capacity.
        let written = unsafe { syar_ui_capture(bytes.as_mut_ptr(), size, &mut width, &mut height) };
        if written != size
            || width <= 0
            || height <= 0
            || width as usize * height as usize * 4 != size
        {
            return Err("Native window changed size during capture.".into());
        }
        let mut encoder = png::Encoder::new(
            std::fs::File::create(path).map_err(|e| e.to_string())?,
            width as u32,
            height as u32,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(&bytes)
            .map_err(|e| e.to_string())
    }
}
