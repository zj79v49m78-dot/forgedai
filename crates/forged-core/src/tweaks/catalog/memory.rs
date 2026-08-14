//! Memory.
//!
//! Worth stating plainly, because the report repeats it: nothing in this section
//! approaches the impact of enabling XMP/EXPO in firmware. If the scan found the
//! RAM running below its rated speed, that BIOS toggle is worth more than every
//! entry here combined. These changes stop Windows making poor paging decisions;
//! they cannot make slow memory fast.

use crate::hardware::{HardwareProfile, MediaType};
use crate::tweaks::model::*;

const MEMORY_MANAGEMENT: &str =
    r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management";

/// True when the system drive is solid state, which changes the correct answer
/// for prefetch and SysMain.
fn system_drive_is_flash(profile: &HardwareProfile) -> bool {
    profile
        .system_drive()
        .is_some_and(|d| matches!(d.media_type, MediaType::Nvme | MediaType::Ssd))
}

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "mem.disable_paging_executive",
        name: "Keep the kernel resident in RAM",
        section: Section::Memory,
        summary: "Sets DisablePagingExecutive so kernel and driver code is never paged to disk.",
        rationale: "By default Windows may page out parts of the kernel under memory pressure. \
                    Paging kernel code back in during a match is a hard stall. With 16 GB or more \
                    there is no reason to allow it.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| p.memory.total_mb >= 8192,
        build: |_| vec![hklm_dword(MEMORY_MANAGEMENT, "DisablePagingExecutive", 1)],
    },
    Tweak {
        id: "mem.large_system_cache_off",
        name: "Keep the large system cache disabled",
        section: Section::Memory,
        summary: "Ensures LargeSystemCache is 0, the correct value for a workstation.",
        rationale: "Setting this to 1 is a widely-copied 'optimisation' that tells Windows to \
                    favour the file cache over application working sets. On a gaming machine that \
                    means the game gets evicted to make room for cached files. This entry enforces \
                    the correct value in case something set it.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| vec![hklm_dword(MEMORY_MANAGEMENT, "LargeSystemCache", 0)],
    },
    Tweak {
        id: "mem.no_pagefile_wipe",
        name: "Stop wiping the page file at shutdown",
        section: Section::Memory,
        summary: "Sets ClearPageFileAtShutdown to 0.",
        rationale: "Overwriting the entire page file on every shutdown is a security measure for \
                    shared machines. It adds tens of seconds to shutdown and does nothing for a \
                    personal gaming PC.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| vec![hklm_dword(MEMORY_MANAGEMENT, "ClearPageFileAtShutdown", 0)],
    },
    Tweak {
        id: "mem.svchost_split_threshold",
        name: "Consolidate service host processes",
        section: Section::Memory,
        summary: "Raises SvcHostSplitThresholdInKB above installed RAM so services share \
                  processes.",
        rationale: "Since Windows 10 1703, machines with more than 3.5 GB run each service in its \
                    own svchost process — often 80 or more of them. Each carries its own overhead \
                    and its own scheduler entry. Consolidating them frees a few hundred megabytes \
                    and reduces context-switch pressure.",
        risk: Risk::Medium,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some(
            "One crashing service can take down others sharing its process. In \
                        practice this is rare, and the affected services restart.",
        ),
        applies_to: |p| p.memory.total_mb >= 8192,
        build: |p| {
            // Threshold must exceed installed RAM in KB for full consolidation.
            let kb = (p.memory.total_mb.max(8192) * 1024).min(u32::MAX as u64) as u32;
            vec![hklm_dword(
                MEMORY_MANAGEMENT,
                "SvcHostSplitThresholdInKB",
                kb,
            )]
        },
    },
    Tweak {
        id: "mem.disable_sysmain",
        name: "Disable SysMain on solid-state storage",
        section: Section::Memory,
        summary: "Stops the SysMain (formerly Superfetch) service.",
        rationale: "SysMain pre-loads frequently used files into spare RAM to hide mechanical \
                    seek time. On NVMe there is no seek time to hide, so it spends CPU and disk \
                    bandwidth preloading data that would have been read fast anyway. Correct to \
                    keep on a hard drive, correct to remove on flash.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: system_drive_is_flash,
        build: |_| vec![service("SysMain", ServiceStart::Disabled)],
    },
    Tweak {
        id: "mem.disable_prefetch",
        name: "Disable prefetch on solid-state storage",
        section: Section::Memory,
        summary: "Turns off the boot and application prefetchers.",
        rationale: "Same reasoning as SysMain: the prefetcher reorders reads to reduce head \
                    movement on a mechanical drive. On flash it is pure overhead.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: system_drive_is_flash,
        build: |_| {
            let path = r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters";
            vec![
                hklm_dword(path, "EnablePrefetcher", 0),
                hklm_dword(path, "EnableSuperfetch", 0),
            ]
        },
    },
    Tweak {
        id: "mem.disable_compression",
        name: "Disable memory compression",
        section: Section::Memory,
        summary: "Turns off the in-memory compression store.",
        rationale: "Memory compression trades CPU cycles for effective RAM capacity. That is the \
                    right trade at 8 GB and the wrong one at 32 GB, where the capacity is not \
                    needed and the CPU time competes with the game.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some(
            "If you later run out of RAM, the machine will page to disk instead of \
                        compressing, which is much slower.",
        ),
        applies_to: |p| p.memory.total_mb >= 16384,
        build: |_| {
            vec![Action::RunCommand {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    "Disable-MMAgent -MemoryCompression".into(),
                ],
                revert: RevertCommand::new(
                    "powershell.exe",
                    &[
                        "-NoProfile",
                        "-Command",
                        "Enable-MMAgent -MemoryCompression",
                    ],
                ),
                tolerate_exit_codes: vec![1],
            }]
        },
    },
    Tweak {
        id: "mem.keep_compression_low_ram",
        name: "Keep memory compression on constrained systems",
        section: Section::Memory,
        summary: "Explicitly leaves compression enabled when the machine has 8 GB or less.",
        rationale: "The mirror of the entry above. At 8 GB, Fortnite plus Windows plus a browser \
                    will exhaust RAM, and compression is what stands between you and paging to \
                    disk. Guides that say 'always disable memory compression' will cost you here.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| p.memory.total_mb > 0 && p.memory.total_mb < 16384,
        build: |_| {
            vec![Action::RunCommand {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    "Enable-MMAgent -MemoryCompression".into(),
                ],
                revert: RevertCommand::new(
                    "powershell.exe",
                    &[
                        "-NoProfile",
                        "-Command",
                        "Enable-MMAgent -MemoryCompression",
                    ],
                ),
                tolerate_exit_codes: vec![1],
            }]
        },
    },
    Tweak {
        id: "mem.pagefile_fixed_size",
        name: "Set a fixed page file",
        section: Section::Memory,
        summary: "Replaces the system-managed page file with a fixed-size one on the system drive.",
        rationale: "A system-managed page file grows on demand, and growing it mid-match is a \
                    disk stall. A fixed size is allocated once and never resized. Sized here at a \
                    conservative multiple of installed RAM rather than the aggressive tiny values \
                    some guides suggest — too small and Fortnite crashes on a memory spike.",
        risk: Risk::Medium,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some("Uses the configured amount of disk permanently."),
        applies_to: |p| p.memory.total_mb >= 8192,
        build: |p| {
            // 8 GB floor, scaling to half of installed RAM, capped at 16 GB.
            let mb = (p.memory.total_mb / 2).clamp(8192, 16384);
            vec![Action::SetRegistry {
                hive: Hive::LocalMachine,
                path: MEMORY_MANAGEMENT.into(),
                value: "PagingFiles".into(),
                data: RegData::MultiSz(vec![format!(r"?:\pagefile.sys {mb} {mb}")]),
            }]
        },
    },
    Tweak {
        id: "mem.disable_pagefile_encryption",
        name: "Disable page file encryption",
        section: Section::Memory,
        summary: "Turns off NTFS encryption of page file contents.",
        rationale: "Encrypting every page written to the page file costs CPU on a path that is \
                    already the slow one. Only meaningful if you are worried about someone \
                    imaging the drive.",
        risk: Risk::Medium,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some(
            "Paged-out memory contents are readable by anyone with physical access to \
                        the drive.",
        ),
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Policies",
                "NtfsEncryptPagingFile",
                0,
            )]
        },
    },
    Tweak {
        id: "mem.io_page_lock_limit",
        name: "I/O page lock limit",
        section: Section::Memory,
        summary: "Sets IoPageLockLimit, a value that has had no effect since Windows XP.",
        rationale:
            "Appears in essentially every 'ultimate Windows tweak' list. The value was \
                    removed from the memory manager two decades ago and is ignored entirely by \
                    modern kernels. Written because people check for it; reported as doing nothing.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::NoMeasuredBenefit,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| vec![hklm_dword(MEMORY_MANAGEMENT, "IoPageLockLimit", 0x10000)],
    },
];
