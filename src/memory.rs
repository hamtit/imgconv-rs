/// Release free heap pages back to the OS (glibc only; a no-op elsewhere).
pub fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` only hands free heap pages back to the OS; it has no preconditions.
    unsafe {
        libc::malloc_trim(0);
    }
}
