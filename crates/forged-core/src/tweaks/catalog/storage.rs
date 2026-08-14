//! Storage.
//!
//! Fortnite streams textures continuously, so storage shows up as traversal
//! stutter rather than as average frame rate. These entries remove filesystem
//! bookkeeping that happens on every access and stop background maintenance
//! from running while you play.

use crate::hardware::{HardwareProfile, MediaType};
use crate::tweaks::model::*;

const FILESYSTEM: &str = r"SYSTEM\CurrentControlSet\Control\FileSystem";

fn has_flash_storage(profile: &HardwareProfile) -> bool {
    profile
        .storage
        .iter()
        .any(|d| matches!(d.media_type, MediaType::Nvme | MediaType::Ssd))
}

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "storage.disable_last_access",
        name: "Stop recording last-access timestamps",
        section: Section::Storage,
        summary: "Disables NTFS last-access time updates.",
        rationale: "Without this, *reading* a file causes a metadata *write*. Fortnite opens \
                    thousands of asset files during a match, so this turns a large stream of \
                    small writes into nothing at all.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| vec![hklm_dword(FILESYSTEM, "NtfsDisableLastAccessUpdate", 0x8000_0001)],
    },
    Tweak {
        id: "storage.disable_8dot3",
        name: "Disable 8.3 short filename creation",
        section: Section::Storage,
        summary: "Stops NTFS generating MS-DOS-compatible short names for new files.",
        rationale: "Every file created gets a second, legacy name computed and stored, which \
                    requires scanning the directory for collisions. Nothing on a modern system \
                    uses these names.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some("Software from the 1990s may fail to find files. Nothing you will run."),
        applies_to: always,
        build: |_| vec![hklm_dword(FILESYSTEM, "NtfsDisable8dot3NameCreation", 1)],
    },
    Tweak {
        id: "storage.disable_search_indexing",
        name: "Disable Windows Search indexing",
        section: Section::Storage,
        summary: "Stops the Windows Search service and its background indexer.",
        rationale: "The indexer crawls the filesystem whenever it judges the machine idle — which \
                    includes menu time between matches. On a machine used only for one game there \
                    is nothing worth indexing.",
        risk: Risk::Medium,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: Some("Start menu file search becomes slow, and searching inside folders no \
                        longer uses an index. Launching apps by name still works."),
        applies_to: always,
        build: |_| vec![service("WSearch", ServiceStart::Disabled)],
    },
    Tweak {
        id: "storage.disable_scheduled_defrag",
        name: "Disable scheduled defragmentation",
        section: Section::Storage,
        summary: "Turns off the weekly Optimize Drives task.",
        rationale: "On flash storage the scheduled task performs a TRIM pass, which is useful, but \
                    it can fire while you are playing. Modern SSDs handle garbage collection \
                    themselves; the scheduled pass is not needed.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: Some("On a mechanical drive, disable this only if you defragment manually."),
        applies_to: has_flash_storage,
        build: |_| {
            vec![command(
                "schtasks.exe",
                &["/Change", "/TN", r"\Microsoft\Windows\Defrag\ScheduledDefrag", "/DISABLE"],
                &["/Change", "/TN", r"\Microsoft\Windows\Defrag\ScheduledDefrag", "/ENABLE"],
            )]
        },
    },
    Tweak {
        id: "storage.ntfs_memory_usage",
        name: "Increase NTFS metadata cache",
        section: Section::Storage,
        summary: "Allows NTFS to use more memory for its metadata cache.",
        rationale: "A larger metadata cache means fewer trips to disk to resolve paths during \
                    Fortnite's asset streaming. Costs a modest amount of RAM.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| p.memory.total_mb >= 16384,
        build: |_| vec![hklm_dword(FILESYSTEM, "NtfsMemoryUsage", 2)],
    },
    Tweak {
        id: "storage.disable_write_cache_buffer_flush",
        name: "Disable write-cache buffer flushing",
        section: Section::Storage,
        summary: "Turns off periodic forced flushes of the drive's write cache.",
        rationale: "Lets the drive decide when to commit its cache rather than being forced to \
                    flush on a schedule, which removes a source of periodic I/O stalls.",
        risk: Risk::High,
        impact: Impact::Minor,
        evidence: Evidence::SituationalGain,
        requires_reboot: true,
        tradeoff: Some("If the machine loses power abruptly, data sitting in the drive's cache is \
                        lost and the filesystem can be left inconsistent. Only reasonable behind \
                        a UPS, or on a machine where losing the OS install is merely annoying."),
        applies_to: has_flash_storage,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Enum\SCSI",
                "UserWriteCacheSetting",
                1,
            )]
        },
    },
    Tweak {
        id: "storage.disable_storage_sense",
        name: "Disable Storage Sense",
        section: Section::Storage,
        summary: "Stops the automatic disk cleanup service.",
        rationale: "Storage Sense deletes temporary files on a schedule, including shader caches. \
                    Deleting Fortnite's shader cache means recompiling it, which is the stutter \
                    you get on the first match after it runs.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: Some("You will need to clear disk space manually."),
        applies_to: always,
        build: |_| {
            vec![hkcu_dword(
                r"Software\Microsoft\Windows\CurrentVersion\StorageSense\Parameters\StoragePolicy",
                "01",
                0,
            )]
        },
    },
    Tweak {
        id: "storage.disable_remote_differential",
        name: "Disable Remote Differential Compression",
        section: Section::Storage,
        summary: "Removes an optional file-transfer compression component.",
        rationale: "A legacy component for efficient file synchronisation over slow links. \
                    Unused on a gaming machine and one less thing loaded at boot.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::NoMeasuredBenefit,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![Action::RunCommand {
                program: "dism.exe".into(),
                args: vec![
                    "/Online".into(),
                    "/Disable-Feature".into(),
                    "/FeatureName:MSRDC-Infrastructure".into(),
                    "/NoRestart".into(),
                ],
                revert: RevertCommand::new(
                    "dism.exe",
                    &[
                        "/Online",
                        "/Enable-Feature",
                        "/FeatureName:MSRDC-Infrastructure",
                        "/NoRestart",
                    ],
                ),
                tolerate_exit_codes: vec![3010, 50],
            }]
        },
    },
];
