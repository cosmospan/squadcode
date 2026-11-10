//! The size of this computer (logical cores and memory), and the concurrency limits that
//! scale with it when a setting is on Auto.

use std::sync::OnceLock;

const GB: u64 = 1 << 30;

/// Logical cores and total memory, rounded to whole gigabytes (0 when it couldn't be read).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hardware {
    pub cores: usize,
    pub ram_gb: u64,
}

/// This computer, read once.
pub fn here() -> Hardware {
    static HERE: OnceLock<Hardware> = OnceLock::new();
    *HERE.get_or_init(|| {
        let hw = Hardware {
            cores: std::thread::available_parallelism().map_or(1, |n| n.get()),
            ram_gb: total_ram().map_or(0, |bytes| (bytes + GB / 2) / GB),
        };
        if hw.ram_gb == 0 {
            crate::applog::info(
                "hardware",
                "couldn't read this computer's memory, so Auto limits stay at their lowest",
            );
        }
        hw
    })
}

/// Crew threads that run at once on Auto. An agent mostly waits on the network and its
/// model, but the builds and tests it runs take cores and memory like anyone's, so one
/// thread per 3 logical cores and per 4 GB, the smaller of the two, from 2 to 8: 8 GB
/// gives 2, 16 GB with 10 cores 3, 32 GB with 12 cores 4, 64 GB with 16 cores 5. Past 8
/// a plan's usage window is the limit, not the computer.
pub fn crew_auto(hw: Hardware) -> usize {
    (hw.cores / 3).min((hw.ram_gb / 4) as usize).clamp(2, 8)
}

/// A concurrency setting's value: "auto" takes `auto`, a number is itself, and 0 is no
/// limit. Anything else is Auto.
pub fn resolve(choice: &str, auto: usize) -> usize {
    choice.trim().parse().unwrap_or(auto)
}

#[cfg(target_os = "macos")]
fn total_ram() -> Option<u64> {
    let mut bytes: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: hw.memsize is a 64-bit integer; the buffer and its length match it.
    let ok = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } == 0;
    (ok && bytes > 0).then_some(bytes)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn total_ram() -> Option<u64> {
    meminfo_total(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// MemTotal from the text of /proc/meminfo, in bytes.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn meminfo_total(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    (kb > 0).then_some(kb * 1024)
}

#[cfg(windows)]
fn total_ram() -> Option<u64> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    // SAFETY: a zeroed MEMORYSTATUSEX with its length set, as the call asks.
    let mut status: MemoryStatusEx = unsafe { std::mem::zeroed() };
    status.length = std::mem::size_of::<MemoryStatusEx>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) } != 0;
    (ok && status.total_phys > 0).then_some(status.total_phys)
}

#[cfg(not(any(unix, windows)))]
fn total_ram() -> Option<u64> {
    None
}

/// How busy the processor is, as a share of all its cores over the last
/// minute (the load average over the core count), 0 to 100.
pub fn busy_pct() -> Option<f32> {
    #[cfg(unix)]
    {
        let mut load = [0f64; 3];
        // SAFETY: getloadavg writes at most `nelem` doubles into the buffer.
        let n = unsafe { libc::getloadavg(load.as_mut_ptr(), 1) };
        if n < 1 {
            return None;
        }
        let cores = here().cores.max(1) as f64;
        Some(((load[0] / cores) * 100.).clamp(0., 100.) as f32)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Free space on the disk that holds `path`, in bytes.
pub fn disk_free(path: &std::path::Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: statvfs fills the struct for a valid, NUL-terminated path.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        #[allow(clippy::unnecessary_cast)]
        Some(st.f_bavail as u64 * st.f_frsize as u64)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// This computer's name as people see it, like "MacBook Pro", read once.
pub fn name() -> Option<String> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(|| {
        #[cfg(target_os = "macos")]
        if let Ok(out) = std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
        {
            let n = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && !n.is_empty() {
                return Some(n);
            }
        }
        #[cfg(unix)]
        {
            let mut buf = [0u8; 256];
            // SAFETY: gethostname writes at most `len` bytes into the buffer.
            if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0 {
                let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
                let n = String::from_utf8_lossy(&buf[..end]).to_string();
                let n = n.trim_end_matches(".local").to_string();
                if !n.is_empty() {
                    return Some(n);
                }
            }
        }
        None
    })
    .clone()
}

/// The system's version, like "macOS 15.2", read once.
pub fn os_version() -> String {
    static OS: OnceLock<String> = OnceLock::new();
    OS.get_or_init(|| {
        #[cfg(target_os = "macos")]
        if let Ok(out) = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
        {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return format!("macOS {v}");
            }
        }
        match std::env::consts::OS {
            "macos" => "macOS".into(),
            "linux" => "Linux".into(),
            "windows" => "Windows".into(),
            other => other.into(),
        }
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(cores: usize, ram_gb: u64) -> Hardware {
        Hardware { cores, ram_gb }
    }

    #[test]
    fn crew_threads_scale_from_two_to_eight() {
        assert_eq!(crew_auto(hw(8, 8)), 2);
        assert_eq!(crew_auto(hw(8, 16)), 2);
        assert_eq!(crew_auto(hw(10, 16)), 3);
        assert_eq!(crew_auto(hw(12, 32)), 4);
        assert_eq!(crew_auto(hw(16, 64)), 5);
        assert_eq!(crew_auto(hw(32, 128)), 8);
        assert_eq!(crew_auto(hw(64, 16)), 4);
        assert_eq!(crew_auto(hw(2, 4)), 2);
        assert_eq!(crew_auto(hw(16, 0)), 2);
    }

    #[test]
    fn choices_resolve_to_a_number() {
        assert_eq!(resolve("auto", 3), 3);
        assert_eq!(resolve("4", 3), 4);
        assert_eq!(resolve("0", 3), 0);
        assert_eq!(resolve("", 3), 3);
        assert_eq!(resolve("lots", 3), 3);
    }

    #[test]
    fn meminfo_gives_total_memory() {
        let text = "MemTotal:       16303428 kB\nMemFree:         1234 kB\n";
        let bytes = meminfo_total(text).unwrap();
        assert_eq!((bytes + GB / 2) / GB, 16);
        assert_eq!(meminfo_total("MemFree: 12 kB\n"), None);
        assert_eq!(meminfo_total("MemTotal: lots\n"), None);
    }

    #[test]
    fn this_computer_reads() {
        let hw = here();
        assert!(hw.cores >= 1);
        if cfg!(any(target_os = "macos", target_os = "linux", windows)) {
            assert!(hw.ram_gb > 0, "{hw:?}");
        }
    }
}
