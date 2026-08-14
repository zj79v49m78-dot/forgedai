//! Firmware recommendations.
//!
//! Forged does not write to firmware. Flashing or scripting BIOS values from
//! Windows means vendor-specific tooling with a real chance of bricking the
//! board, and there is no undo for a bad firmware write. So this module produces
//! a checklist the user applies by hand, with menu locations tailored to the
//! detected motherboard vendor.
//!
//! The AI planner normally produces this list, because it can tailor the wording
//! to the exact board. [`deterministic_recommendations`] is the offline
//! equivalent, and also seeds the planner with the findings that must not be
//! missed.

use crate::ai::{BiosPriority, BiosRecommendation};
use crate::hardware::HardwareProfile;

/// Vendor-specific menu hints. Board firmware differs enough that a generic
/// "look in Advanced" is nearly useless, and a wrong-but-specific hint is worse
/// than an honest generic one — so unknown vendors get the generic text.
fn vendor_hint(profile: &HardwareProfile, topic: Topic) -> String {
    let vendor = profile.motherboard.manufacturer.to_uppercase();

    let asus = vendor.contains("ASUS") || vendor.contains("ASUSTEK");
    let msi = vendor.contains("MSI") || vendor.contains("MICRO-STAR");
    let gigabyte = vendor.contains("GIGABYTE");
    let asrock = vendor.contains("ASROCK");

    match topic {
        Topic::MemoryProfile => {
            if asus {
                "Ai Tweaker → Ai Overclock Tuner → XMP I / EXPO I".into()
            } else if msi {
                "OC → Extreme Memory Profile (XMP) / A-XMP".into()
            } else if gigabyte {
                "Tweaker → Extreme Memory Profile (X.M.P.)".into()
            } else if asrock {
                "OC Tweaker → Load XMP Setting / EXPO".into()
            } else {
                "Look for XMP, EXPO, DOCP or A-XMP in the overclocking or memory menu".into()
            }
        }
        Topic::ResizableBar => {
            if asus {
                "Advanced → PCI Subsystem Settings → Above 4G Decoding + Re-Size BAR Support".into()
            } else if msi {
                "Settings → Advanced → PCI Subsystem Settings → Above 4G memory / Re-Size BAR"
                    .into()
            } else if gigabyte {
                "Settings → IO Ports → Above 4G Decoding + Re-Size BAR Support".into()
            } else if asrock {
                "Advanced → Chipset Configuration → Above 4G Decoding + Re-Size BAR".into()
            } else {
                "Look for 'Above 4G Decoding' and 'Resizable BAR' under PCI or chipset settings"
                    .into()
            }
        }
        Topic::CStates => {
            if asus {
                "Advanced → CPU Configuration → CPU Power Management".into()
            } else if msi {
                "OC → Advanced CPU Configuration → CPU Features".into()
            } else if gigabyte {
                "Settings → Platform Power / Tweaker → Advanced CPU Settings".into()
            } else {
                "Look for C-State Control or CPU Power Management under CPU configuration".into()
            }
        }
        Topic::FastBoot => "Boot menu → Fast Boot".into(),
        Topic::Virtualisation => {
            if asus {
                "Advanced → CPU Configuration → SVM Mode (AMD) / Intel Virtualization Technology"
                    .into()
            } else {
                "CPU configuration → SVM Mode (AMD) or Intel VT-x".into()
            }
        }
        Topic::Fclk => "Ai Tweaker / OC → Infinity Fabric Frequency (FCLK)".into(),
        Topic::PowerLimits => {
            if asus {
                "Ai Tweaker → Internal CPU Power Management → Long/Short Duration Package Power Limit".into()
            } else {
                "CPU power management → PL1 / PL2 / PPT limits".into()
            }
        }
    }
}

enum Topic {
    MemoryProfile,
    ResizableBar,
    CStates,
    FastBoot,
    Virtualisation,
    Fclk,
    PowerLimits,
}

/// The firmware checklist derived purely from the scan.
pub fn deterministic_recommendations(profile: &HardwareProfile) -> Vec<BiosRecommendation> {
    let mut out = Vec::new();

    // ---- Memory profile: the single highest-value firmware change ----------
    if profile.memory.xmp_appears_disabled() {
        out.push(BiosRecommendation {
            setting: "XMP / EXPO memory profile".into(),
            target_value: format!("Enable — Profile 1 ({} MT/s)", profile.memory.rated_mhz),
            reason: format!(
                "Your memory is rated for {} MT/s but is currently running at {} MT/s, the JEDEC \
                 fallback speed. Fortnite's 1% lows are strongly tied to memory bandwidth and \
                 latency; enabling the rated profile is worth more than every software change \
                 Forged just made, combined. If the machine fails to boot afterwards, clear CMOS \
                 and try Profile 2 or a manually lower speed.",
                profile.memory.rated_mhz, profile.memory.configured_mhz
            ),
            where_to_find: vendor_hint(profile, Topic::MemoryProfile),
            priority: BiosPriority::Critical,
        });
    } else if profile.memory.configured_mhz > 0 {
        out.push(BiosRecommendation {
            setting: "XMP / EXPO memory profile".into(),
            target_value: "Already enabled — no action needed".into(),
            reason: format!(
                "Memory is running at its rated {} MT/s. Nothing to do here.",
                profile.memory.configured_mhz
            ),
            where_to_find: vendor_hint(profile, Topic::MemoryProfile),
            priority: BiosPriority::Optional,
        });
    }

    // ---- Resizable BAR ----------------------------------------------------
    if let Some(gpu) = profile.primary_gpu() {
        if !gpu.is_integrated {
            out.push(BiosRecommendation {
                setting: "Above 4G Decoding + Resizable BAR".into(),
                target_value: "Both Enabled".into(),
                reason: "Lets the CPU address the whole framebuffer at once instead of through a \
                         256 MB window. Typically worth a few percent, occasionally more. Above 4G \
                         Decoding must be enabled first or the Resizable BAR option stays hidden."
                    .into(),
                where_to_find: vendor_hint(profile, Topic::ResizableBar),
                priority: BiosPriority::Recommended,
            });
        }
    }

    // ---- Memory channel population ----------------------------------------
    if profile.memory.appears_single_channel() {
        out.push(BiosRecommendation {
            setting: "Memory channel configuration".into(),
            target_value: "Populate a second matching module".into(),
            reason:
                "Only one memory module was detected, so the machine is running single-channel \
                     and has half the memory bandwidth it could. This is a hardware change rather \
                     than a setting — add a second identical module in the slot your manual \
                     specifies for dual-channel, usually A2/B2."
                    .into(),
            where_to_find: "Physical — check the motherboard manual for correct slot pairing"
                .into(),
            priority: BiosPriority::Critical,
        });
    }

    // ---- C-states ---------------------------------------------------------
    out.push(BiosRecommendation {
        setting: "CPU C-States (package power states)".into(),
        target_value: "Disabled — but only if temperatures allow".into(),
        reason: "Deep sleep states take time to exit and flush cache, which shows up as \
                 frame-time inconsistency. Disabling them keeps the CPU responsive at the cost of \
                 running hot permanently. Apply this one *after* checking your idle temperatures: \
                 if the CPU already idles above about 50 °C, leave C-States alone — thermal \
                 throttling will cost you more than the tweak gains."
            .into(),
        where_to_find: vendor_hint(profile, Topic::CStates),
        priority: BiosPriority::Optional,
    });

    // ---- Fast Boot --------------------------------------------------------
    out.push(BiosRecommendation {
        setting: "Fast Boot".into(),
        target_value: "Disabled".into(),
        reason: "Fast Boot skips USB controller initialisation during POST, which is why some \
                 keyboards and mice do not respond until Windows loads — and why you sometimes \
                 cannot get into BIOS. Costs a few seconds at boot and removes a class of \
                 peripheral problems."
            .into(),
        where_to_find: vendor_hint(profile, Topic::FastBoot),
        priority: BiosPriority::Recommended,
    });

    // ---- Virtualisation ---------------------------------------------------
    if profile.os.vbs_enabled || profile.cpu.virtualization_enabled {
        out.push(BiosRecommendation {
            setting: "SVM Mode / Intel VT-x".into(),
            target_value: "Disabled".into(),
            reason: "Forged disabled Virtualisation Based Security in Windows. Turning the \
                     hardware virtualisation extension off in firmware as well guarantees VBS \
                     cannot silently re-enable itself after a feature update. Skip this if you use \
                     WSL, Docker, virtual machines, or an Android emulator — all of them need it."
                .into(),
            where_to_find: vendor_hint(profile, Topic::Virtualisation),
            priority: BiosPriority::Optional,
        });
    }

    // ---- AMD Infinity Fabric ----------------------------------------------
    if profile.cpu.is_amd_zen() && profile.memory.rated_mhz > 0 {
        // FCLK is conventionally half the memory data rate for 1:1 operation.
        let target_fclk = profile.memory.rated_mhz / 2;
        out.push(BiosRecommendation {
            setting: "Infinity Fabric Clock (FCLK)".into(),
            target_value: format!("{target_fclk} MHz — synchronous with memory"),
            reason: format!(
                "On Ryzen, memory latency is lowest when the Infinity Fabric runs in a 1:1 ratio \
                 with the memory controller. For your {} MT/s kit that means {} MHz FCLK. If the \
                 board has set a different ratio automatically, latency is higher than it needs to \
                 be. Above roughly 3600 MT/s many chips cannot hold 1:1 — if it will not post, \
                 leave FCLK on Auto.",
                profile.memory.rated_mhz, target_fclk
            ),
            where_to_find: vendor_hint(profile, Topic::Fclk),
            priority: BiosPriority::Recommended,
        });
    }

    // ---- Power limits -----------------------------------------------------
    if profile.cpu.physical_cores >= 8 {
        out.push(BiosRecommendation {
            setting: "CPU power limits (PL1 / PL2 or PPT)".into(),
            target_value: "Leave at motherboard defaults unless thermally limited".into(),
            reason: "Many boards ship with power limits unlocked well beyond the chip's rated \
                     envelope, which produces heat rather than frames in a game that does not use \
                     all cores. If your CPU is hitting thermal limits during play, setting the \
                     limits to the manufacturer's specification will *improve* sustained clocks."
                .into(),
            where_to_find: vendor_hint(profile, Topic::PowerLimits),
            priority: BiosPriority::Optional,
        });
    }

    // ---- Storage ----------------------------------------------------------
    if profile
        .storage
        .iter()
        .any(|d| d.hosts_fortnite && d.media_type == crate::hardware::MediaType::Hdd)
    {
        out.push(BiosRecommendation {
            setting: "Storage — move the game install".into(),
            target_value: "Reinstall Fortnite onto an SSD or NVMe drive".into(),
            reason: "Fortnite is installed on a mechanical hard drive. Texture streaming from \
                     spinning media is the direct cause of traversal stutter, and no firmware or \
                     registry setting can compensate for it."
                .into(),
            where_to_find: "Epic Games Launcher → Library → Fortnite → ⋯ → Move".into(),
            priority: BiosPriority::Critical,
        });
    }

    out
}

/// Renders the checklist as Markdown, for the "copy to clipboard" button and
/// the exported report.
pub fn to_markdown(recommendations: &[BiosRecommendation], profile: &HardwareProfile) -> String {
    let mut out = String::new();

    out.push_str("# BIOS settings to change by hand\n\n");
    out.push_str(&format!(
        "Motherboard: **{} {}** · BIOS version {} ({})\n\n",
        profile.motherboard.manufacturer,
        profile.motherboard.product,
        profile.motherboard.bios_version,
        profile.motherboard.bios_release_date
    ));
    out.push_str(
        "Forged does not write to firmware — a bad firmware write has no undo. Apply these \
         yourself. Enter BIOS by pressing Delete or F2 repeatedly during boot.\n\n",
    );

    for priority in [
        BiosPriority::Critical,
        BiosPriority::Recommended,
        BiosPriority::Optional,
    ] {
        let group: Vec<_> = recommendations
            .iter()
            .filter(|r| r.priority == priority)
            .collect();
        if group.is_empty() {
            continue;
        }

        let heading = match priority {
            BiosPriority::Critical => "## Critical — do these first",
            BiosPriority::Recommended => "## Recommended",
            BiosPriority::Optional => "## Optional — read the caveat before applying",
        };
        out.push_str(heading);
        out.push_str("\n\n");

        for rec in group {
            out.push_str(&format!("### {}\n\n", rec.setting));
            out.push_str(&format!("**Set to:** {}\n\n", rec.target_value));
            out.push_str(&format!("**Where:** {}\n\n", rec.where_to_find));
            out.push_str(&format!("{}\n\n", rec.reason));
        }
    }

    out.push_str(
        "---\n\n*If the machine will not boot after a change, clear CMOS — either the jumper on \
         the board or by removing the coin cell for a minute. That reverts every firmware change \
         and costs nothing.*\n",
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{Memory, Motherboard};

    #[test]
    fn xmp_disabled_produces_a_critical_recommendation() {
        let profile = HardwareProfile {
            memory: Memory {
                configured_mhz: 2133,
                rated_mhz: 3600,
                total_mb: 16384,
                module_count: 2,
                ..Default::default()
            },
            ..Default::default()
        };

        let recs = deterministic_recommendations(&profile);
        let xmp = recs
            .iter()
            .find(|r| r.setting.contains("XMP"))
            .expect("XMP recommendation missing");
        assert_eq!(xmp.priority, BiosPriority::Critical);
        assert!(xmp.reason.contains("3600"));
        assert!(xmp.reason.contains("2133"));
    }

    #[test]
    fn xmp_already_enabled_is_not_critical() {
        let profile = HardwareProfile {
            memory: Memory {
                configured_mhz: 3600,
                rated_mhz: 3600,
                total_mb: 16384,
                module_count: 2,
                ..Default::default()
            },
            ..Default::default()
        };

        let recs = deterministic_recommendations(&profile);
        let xmp = recs.iter().find(|r| r.setting.contains("XMP")).unwrap();
        assert_eq!(xmp.priority, BiosPriority::Optional);
    }

    #[test]
    fn vendor_hints_are_board_specific() {
        let asus = HardwareProfile {
            motherboard: Motherboard {
                manufacturer: "ASUSTeK COMPUTER INC.".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(vendor_hint(&asus, Topic::MemoryProfile).contains("Ai Tweaker"));

        let msi = HardwareProfile {
            motherboard: Motherboard {
                manufacturer: "Micro-Star International Co., Ltd.".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(vendor_hint(&msi, Topic::MemoryProfile).contains("OC →"));
    }

    #[test]
    fn unknown_vendor_gets_generic_but_useful_text() {
        let unknown = HardwareProfile::default();
        let hint = vendor_hint(&unknown, Topic::MemoryProfile);
        assert!(
            hint.contains("XMP"),
            "generic hint should still name the setting"
        );
    }

    #[test]
    fn amd_gets_a_fabric_recommendation_at_the_right_ratio() {
        let profile = HardwareProfile {
            cpu: crate::hardware::Cpu {
                vendor: crate::hardware::CpuVendor::Amd,
                name: "AMD Ryzen 5 7600X".into(),
                ..Default::default()
            },
            memory: Memory {
                rated_mhz: 3600,
                configured_mhz: 3600,
                ..Default::default()
            },
            ..Default::default()
        };

        let recs = deterministic_recommendations(&profile);
        let fclk = recs.iter().find(|r| r.setting.contains("FCLK")).unwrap();
        assert!(fclk.target_value.contains("1800"));
    }

    #[test]
    fn intel_gets_no_fabric_recommendation() {
        let profile = HardwareProfile {
            cpu: crate::hardware::Cpu {
                vendor: crate::hardware::CpuVendor::Intel,
                ..Default::default()
            },
            memory: Memory {
                rated_mhz: 3600,
                configured_mhz: 3600,
                ..Default::default()
            },
            ..Default::default()
        };
        let recs = deterministic_recommendations(&profile);
        assert!(!recs.iter().any(|r| r.setting.contains("FCLK")));
    }

    #[test]
    fn markdown_groups_by_priority() {
        let profile = HardwareProfile {
            memory: Memory {
                configured_mhz: 2133,
                rated_mhz: 3600,
                total_mb: 16384,
                module_count: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = to_markdown(&deterministic_recommendations(&profile), &profile);
        assert!(md.contains("## Critical"));
        assert!(md.contains("clear CMOS"));
    }
}
