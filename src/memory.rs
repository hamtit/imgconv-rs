use std::sync::Once;

static INIT: Once = Once::new();

/// Default `M_MMAP_THRESHOLD`: allocations this large or larger are mmapped,
/// so their memory goes back to the OS on free. Setting it explicitly also
/// stops glibc from raising the threshold dynamically.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const DEFAULT_MMAP_THRESHOLD: i32 = 256 * 1024;
/// Default `M_ARENA_MAX`: caps the number of per-thread malloc arenas.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const DEFAULT_ARENA_MAX: i32 = 4;

/// Tune the allocator once, before the first conversion (glibc only; a no-op
/// elsewhere). Overrides, read from the environment:
/// `IMGCONV_MMAP_THRESHOLD` (bytes) and `IMGCONV_ARENA_MAX`. A value of 0
/// leaves that parameter alone.
pub fn init() {
    INIT.call_once(|| {
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        {
            let threshold = env_value("IMGCONV_MMAP_THRESHOLD", DEFAULT_MMAP_THRESHOLD);
            let arenas = env_value("IMGCONV_ARENA_MAX", DEFAULT_ARENA_MAX);
            unsafe {
                if threshold > 0 {
                    libc::mallopt(libc::M_MMAP_THRESHOLD, threshold);
                }
                if arenas > 0 {
                    libc::mallopt(libc::M_ARENA_MAX, arenas);
                }
            }
        }
    });
}

/// `name` from the environment as an i32, or `default` when unset or invalid.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn env_value(name: &str, default: i32) -> i32 {
    match std::env::var(name).ok().and_then(|v| v.trim().parse::<u64>().ok()) {
        Some(v) => i32::try_from(v).unwrap_or(i32::MAX),
        None => default,
    }
}

/// Release free heap pages back to the OS (glibc only; a no-op elsewhere).
pub fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
}
