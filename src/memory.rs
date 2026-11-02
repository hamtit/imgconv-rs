/// Release free heap pages back to the OS (glibc only; a no-op elsewhere).
pub fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
}
