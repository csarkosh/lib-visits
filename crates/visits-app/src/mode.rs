//! What the program may record and send, from the person's settings, the
//! environment and the build (the design's 4.2).

use crate::report::Channel;

/// Whether usage is recorded, and whether it may leave the machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Usage {
    /// Nothing is recorded.
    Off,
    /// Recorded on the person's disk; nothing leaves.
    Local,
    /// Recorded and sent.
    On,
}

/// What happens to a crash report.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Crashes {
    Never,
    Ask,
    Always,
}

/// The person's own choices, `None` where they have not made one.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    pub usage: Option<Usage>,
    pub crashes: Option<Crashes>,
}

/// The mode the program runs in. `uploads` says whether reports may leave the
/// machine at all; `invite` whether to show the invitation to turn usage on;
/// `log` whether to print each payload to stderr instead of sending it;
/// `reason` names the rule that kept `uploads` off, when one did.
#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    pub usage: Usage,
    pub crashes: Crashes,
    pub uploads: bool,
    pub invite: bool,
    pub log: bool,
    pub reason: Option<&'static str>,
}

/// The mode for these settings. `env` reads an environment variable and
/// `app_env_var` names the program's own (`ARKOS_TELEMETRY`); `interactive`
/// is whether a person is at the terminal or the window.
///
/// The rules, in order, the first match naming the reason that keeps uploads off:
/// 1. a source build (`source build`);
/// 2. `DO_NOT_TRACK` set to anything but empty, `0` or `false` (`DO_NOT_TRACK`),
///    which also turns usage `Off` and crashes `Never`;
/// 3. `app_env_var` equal to `off` (`telemetry off`), the same as rule 2, or to
///    `log` (`telemetry log`), which keeps the settings and sets `log`;
/// 4. `CI` set (`CI`), or not interactive (`not interactive`), where local
///    recording continues;
/// 5. otherwise the settings, `Local` and `Ask` where they say nothing, with
///    uploads only for `On`.
///
/// The effects of rules 2 and 3 on usage and logging hold even when an earlier
/// rule names the reason, so a source build with `DO_NOT_TRACK` records nothing
/// and a source build with `log` prints what it would send.
pub fn mode(
    settings: &Settings,
    env: &dyn Fn(&str) -> Option<String>,
    app_env_var: &str,
    channel: &Channel,
    interactive: bool,
) -> Mode {
    let var = |k: &str| env(k).unwrap_or_default();
    let dnt = !matches!(var("DO_NOT_TRACK").as_str(), "" | "0" | "false");
    let telemetry = var(app_env_var);
    let off = telemetry == "off";
    let log = telemetry == "log" && !dnt;

    let (usage, crashes) = if dnt || off {
        (Usage::Off, Crashes::Never)
    } else {
        (settings.usage.unwrap_or(Usage::Local), settings.crashes.unwrap_or(Crashes::Ask))
    };

    let reason = if channel.is_source() {
        Some("source build")
    } else if dnt {
        Some("DO_NOT_TRACK")
    } else if off {
        Some("telemetry off")
    } else if log {
        Some("telemetry log")
    } else if env("CI").is_some() {
        Some("CI")
    } else if !interactive {
        Some("not interactive")
    } else {
        None
    };

    Mode {
        usage,
        crashes,
        uploads: reason.is_none() && usage == Usage::On,
        invite: reason.is_none() && usage == Usage::Local && !channel.is_source(),
        log,
        reason,
    }
}
