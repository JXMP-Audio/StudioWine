//! Hyprland window rules for Windows programs, added at run time through `hyprctl`
//! (nothing is written to the user's config; a config reload drops them).
//!
//! Cubase 15 draws the events being dragged into an untitled, mostly transparent window
//! over the whole project window. Hyprland animates every new window in and blurs what
//! lies behind transparent pixels, so each drag blurred the arrangement. This holds even
//! when Wine makes the window override-redirect: Hyprland still lists and decorates it.
//! Untitled Wine windows are overlays, menus and tooltips, none of which should be
//! animated, blurred or shadowed.
use crate::log::LogSink;
use std::process::Command;

const RULE_NAME: &str = "studiowine-untitled-overlays";

/// `hyprctl` argument lists to try in order; the first one answered with `ok` wins.
/// Hyprland 0.55 with a Lua config takes `eval` and refuses `keyword`; older releases
/// parse the hyprlang form.
fn attempts() -> Vec<Vec<String>> {
    let lua = format!(
        r#"hl.window_rule({{ name = "{RULE_NAME}", match = {{ class = "\\.exe$", title = "^$", xwayland = true }}, no_anim = true, no_blur = true, no_shadow = true, no_dim = true }})"#
    );
    vec![
        vec!["eval".into(), lua],
        vec![
            "keyword".into(),
            "windowrule".into(),
            r"match:class \.exe$, match:title ^$, match:xwayland 1, no_anim on, no_blur on, no_shadow on, no_dim on".into(),
        ],
        vec!["keyword".into(), "windowrulev2".into(), r"noanim, class:\.exe$, title:^$, xwayland:1".into()],
        vec!["keyword".into(), "windowrulev2".into(), r"noblur, class:\.exe$, title:^$, xwayland:1".into()],
        vec!["keyword".into(), "windowrulev2".into(), r"noshadow, class:\.exe$, title:^$, xwayland:1".into()],
    ]
}

/// Adds the rules when running under Hyprland; does nothing elsewhere. Adding them again
/// is harmless, so this runs before every program start (a config reload drops them).
pub fn apply_window_rules(log: &LogSink) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none_or(|s| s.is_empty()) {
        return;
    }
    let mut legacy_ok = false;
    for args in attempts() {
        let out = match Command::new("hyprctl").args(&args).output() {
            Ok(o) => o,
            Err(e) => {
                log.line(&format!("hyprctl not usable, no window rules added: {e}"));
                return;
            }
        };
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let ok = out.status.success() && text == "ok";
        if args[1] == "windowrulev2" {
            // The three legacy rules belong together.
            legacy_ok |= ok;
            continue;
        }
        if ok {
            log.line(&format!("Hyprland window rules added (hyprctl {})", args[0]));
            return;
        }
    }
    if legacy_ok {
        log.line("Hyprland window rules added (hyprctl keyword windowrulev2)");
    } else {
        log.line("Hyprland did not accept StudioWine's window rules; overlays may be animated and blurred");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_rule_is_one_valid_call() {
        let lua = &attempts()[0][1];
        assert!(lua.starts_with("hl.window_rule({ name = \"studiowine-untitled-overlays\""), "{lua}");
        // Lua sees `\.exe$`: a literal dot, so `notexe` never matches.
        assert!(lua.contains(r#"class = "\\.exe$""#), "{lua}");
        assert!(lua.contains(r#"title = "^$""#), "{lua}");
        assert_eq!(lua.matches('{').count(), lua.matches('}').count(), "{lua}");
    }
}
