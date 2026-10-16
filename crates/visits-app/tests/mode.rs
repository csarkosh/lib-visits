use visits_app::{mode, Channel, Crashes, Mode, Settings, Usage};

const VAR: &str = "ARKOS_TELEMETRY";

/// The mode for these settings, environment and channel, interactive or not.
fn run(settings: &Settings, vars: &[(&str, &str)], ch: &Channel, interactive: bool) -> Mode {
    let env = |k: &str| vars.iter().find(|(name, _)| *name == k).map(|(_, v)| v.to_string());
    mode(settings, &env, VAR, ch, interactive)
}

fn release(settings: &Settings, vars: &[(&str, &str)]) -> Mode {
    run(settings, vars, &Channel::Release, true)
}

fn on() -> Settings {
    Settings { usage: Some(Usage::On), crashes: Some(Crashes::Always) }
}

#[test]
fn an_empty_settings_records_locally_and_invites() {
    let m = release(&Settings::default(), &[]);
    assert_eq!(
        m,
        Mode { usage: Usage::Local, crashes: Crashes::Ask, uploads: false, invite: true, log: false, reason: None }
    );
}

#[test]
fn an_empty_settings_never_uploads_whatever_the_environment() {
    let envs: &[&[(&str, &str)]] = &[&[], &[("DO_NOT_TRACK", "0")], &[("DO_NOT_TRACK", "false")], &[(VAR, "on")], &[(VAR, "")], &[("CI", "")]];
    for vars in envs {
        for ch in [Channel::Release, Channel::Source, Channel::Named("tap".into())] {
            for interactive in [true, false] {
                assert!(!run(&Settings::default(), vars, &ch, interactive).uploads, "{vars:?} {ch:?} {interactive}");
            }
        }
    }
}

#[test]
fn settings_on_uploads() {
    let m = release(&on(), &[]);
    assert_eq!(
        m,
        Mode { usage: Usage::On, crashes: Crashes::Always, uploads: true, invite: false, log: false, reason: None }
    );
}

#[test]
fn settings_off_records_nothing_and_does_not_invite() {
    let m = release(&Settings { usage: Some(Usage::Off), crashes: Some(Crashes::Never) }, &[]);
    assert_eq!(
        m,
        Mode { usage: Usage::Off, crashes: Crashes::Never, uploads: false, invite: false, log: false, reason: None }
    );
}

#[test]
fn a_named_channel_follows_the_settings() {
    let m = run(&on(), &[], &Channel::Named("tap".into()), true);
    assert!(m.uploads);
    assert_eq!(m.reason, None);
    assert!(run(&Settings::default(), &[], &Channel::Named("tap".into()), true).invite);
}

#[test]
fn a_source_channel_never_uploads_nor_invites() {
    let m = run(&on(), &[], &Channel::Source, true);
    assert!(!m.uploads);
    assert!(!m.invite);
    assert_eq!(m.reason, Some("source build"));
    let m = run(&Settings::default(), &[], &Channel::Source, true);
    assert!(!m.uploads);
    assert!(!m.invite);
    assert_eq!(m.reason, Some("source build"));
}

#[test]
fn a_named_source_channel_is_a_source_build() {
    let m = run(&on(), &[], &Channel::Named("source".into()), true);
    assert!(!m.uploads);
    assert_eq!(m.reason, Some("source build"));
}

#[test]
fn source_wins_over_every_later_rule() {
    let m = run(&on(), &[("DO_NOT_TRACK", "1"), (VAR, "off"), ("CI", "true")], &Channel::Source, false);
    assert_eq!(m.reason, Some("source build"));
    assert!(!m.uploads);
}

#[test]
fn do_not_track_one_turns_everything_off() {
    let m = release(&on(), &[("DO_NOT_TRACK", "1")]);
    assert_eq!(
        m,
        Mode { usage: Usage::Off, crashes: Crashes::Never, uploads: false, invite: false, log: false, reason: Some("DO_NOT_TRACK") }
    );
}

#[test]
fn do_not_track_anything_turns_everything_off() {
    let m = release(&on(), &[("DO_NOT_TRACK", "anything")]);
    assert_eq!((m.usage, m.crashes, m.uploads, m.reason), (Usage::Off, Crashes::Never, false, Some("DO_NOT_TRACK")));
}

#[test]
fn do_not_track_zero_does_not_disable() {
    assert_eq!(release(&on(), &[("DO_NOT_TRACK", "0")]), release(&on(), &[]));
}

#[test]
fn do_not_track_false_does_not_disable() {
    assert_eq!(release(&on(), &[("DO_NOT_TRACK", "false")]), release(&on(), &[]));
}

#[test]
fn do_not_track_empty_does_not_disable() {
    assert_eq!(release(&on(), &[("DO_NOT_TRACK", "")]), release(&on(), &[]));
}

#[test]
fn do_not_track_wins_over_the_app_variable_and_ci() {
    let m = release(&on(), &[("DO_NOT_TRACK", "1"), (VAR, "log"), ("CI", "1")]);
    assert_eq!(m.reason, Some("DO_NOT_TRACK"));
    assert_eq!(m.usage, Usage::Off);
    assert!(!m.log);
}

#[test]
fn telemetry_off_turns_everything_off() {
    let m = release(&on(), &[(VAR, "off")]);
    assert_eq!(
        m,
        Mode { usage: Usage::Off, crashes: Crashes::Never, uploads: false, invite: false, log: false, reason: Some("telemetry off") }
    );
}

#[test]
fn telemetry_log_keeps_the_settings_and_logs_instead_of_uploading() {
    let m = release(&on(), &[(VAR, "log")]);
    assert_eq!(
        m,
        Mode { usage: Usage::On, crashes: Crashes::Always, uploads: false, invite: false, log: true, reason: Some("telemetry log") }
    );
}

#[test]
fn telemetry_log_wins_over_ci() {
    let m = run(&on(), &[(VAR, "log"), ("CI", "1")], &Channel::Release, false);
    assert_eq!(m.reason, Some("telemetry log"));
    assert!(m.log);
    assert!(!m.uploads);
}

#[test]
fn the_app_variable_turns_off_without_regard_to_case() {
    for off in ["off", "OFF", "Off", "0", "false", "FALSE", "no", "NO"] {
        let m = release(&on(), &[(VAR, off)]);
        assert_eq!(
            m,
            Mode { usage: Usage::Off, crashes: Crashes::Never, uploads: false, invite: false, log: false, reason: Some("telemetry off") },
            "{off}"
        );
    }
}

#[test]
fn the_app_variable_logs_without_regard_to_case() {
    for log in ["log", "LOG", "Log"] {
        let m = release(&on(), &[(VAR, log)]);
        assert_eq!(
            m,
            Mode { usage: Usage::On, crashes: Crashes::Always, uploads: false, invite: false, log: true, reason: Some("telemetry log") },
            "{log}"
        );
    }
}

#[test]
fn another_app_variable_value_changes_nothing() {
    for other in ["on", "ON", "1", "true", "yes", "", "offf", " off", "logs"] {
        assert_eq!(release(&on(), &[(VAR, other)]), release(&on(), &[]), "{other:?}");
    }
    assert_eq!(release(&on(), &[("OTHER_TELEMETRY", "off")]), release(&on(), &[]));
}

#[test]
fn ci_keeps_local_recording_without_uploads() {
    let m = release(&Settings::default(), &[("CI", "true")]);
    assert_eq!(
        m,
        Mode { usage: Usage::Local, crashes: Crashes::Ask, uploads: false, invite: false, log: false, reason: Some("CI") }
    );
    let m = release(&on(), &[("CI", "1")]);
    assert_eq!((m.usage, m.uploads, m.reason), (Usage::On, false, Some("CI")));
}

#[test]
fn ci_empty_is_as_if_unset() {
    let m = release(&on(), &[("CI", "")]);
    assert_eq!(m, release(&on(), &[]));
    assert!(m.uploads);
    assert_eq!(m.reason, None);
}

#[test]
fn ci_wins_over_not_interactive() {
    let m = run(&Settings::default(), &[("CI", "1")], &Channel::Release, false);
    assert_eq!(m.reason, Some("CI"));
}

#[test]
fn not_interactive_keeps_recording_without_uploads_or_invitation() {
    let m = run(&on(), &[], &Channel::Release, false);
    assert_eq!(
        m,
        Mode { usage: Usage::On, crashes: Crashes::Always, uploads: false, invite: false, log: false, reason: Some("not interactive") }
    );
    let m = run(&Settings::default(), &[], &Channel::Release, false);
    assert_eq!((m.usage, m.uploads, m.invite, m.reason), (Usage::Local, false, false, Some("not interactive")));
}

#[test]
fn local_with_a_crash_choice_still_invites() {
    let m = release(&Settings { usage: Some(Usage::Local), crashes: Some(Crashes::Never) }, &[]);
    assert_eq!(
        m,
        Mode { usage: Usage::Local, crashes: Crashes::Never, uploads: false, invite: true, log: false, reason: None }
    );
}
