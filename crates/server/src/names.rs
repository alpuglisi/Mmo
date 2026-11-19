//! One filter for every name a player chooses and others see: lineage (and
//! design) names, species names, nameplate titles, and new account names
//! (playtests 3 and 4: bidi overrides, zero-width characters, markup and
//! "Server Admin" were accepted, and a lineage name became its founding
//! species' name past the species filter).
//!
//! A name is ASCII letters, digits, single spaces and a little punctuation
//! ([`PUNCTUATION`]): no look-alike letters from other scripts, nothing
//! that reads as markup. It may not pass for the server or its staff
//! ([`reserved`], matched through look-alike folding, [`fold`]). Callers
//! with the players at hand also refuse names that look like another
//! player's ([`impersonates`]). Names saved before the filter are cleaned
//! when a world is loaded ([`sanitize`]).

/// Punctuation a name may use besides letters, digits and single spaces.
pub const PUNCTUATION: &str = ".,'!?-:+#()_";

/// Folded fragments a name may not contain anywhere: they would pass for
/// the server or its staff.
const RESERVED_ANYWHERE: &[&str] = &[
    "admin",
    "moderator",
    "gamemaster",
    "gamemod",
    "sysop",
    "server",
    "staff",
    "system",
    "official",
    "developer",
    "anthropic",
    // Misspellings that read the same (playtest 6).
    "moderater",
    "gamemastr",
    "rootuser",
];

/// Words a name may not use as a whole word (folded): too short or too
/// common to match inside other words ("model", "modest", "owners'…").
const RESERVED_WORDS: &[&str] = &[
    "gm",
    "gms",
    "mod",
    "mods",
    "dev",
    "devs",
    "owner",
    "owners",
    "support",
    "helpdesk",
    "claude",
    "operator",
    "operators",
];

/// What a whole name may not come down to, once folded with its spaces and
/// punctuation dropped, a leading "the", a trailing team word and trailing
/// digits taken off (playtest 5: "G.M.", "M.o.d", "TheOwner",
/// "SupportTeam", "Help desk" and "wild" all passed): the staff words, the
/// wild (the owner of every wild animal) and the root account.
const RESERVED_WHOLE: &[&str] =
    &["gm", "mod", "dev", "owner", "support", "helpdesk", "help", "claude", "wild", "root", "admin", "op", "ops"];

/// Words a staff name may end in ("SupportTeam", "DevCrew", "Mods").
const TEAM_SUFFIXES: &[&str] = &["team", "crew", "squad", "s"];

/// Whole forms that are also plain words whose plural is just a plural
/// ("Roots", "Helps"): no `s` suffix for them (playtest 6: "Roots" refused).
const PLAIN_PLURALS: &[&str] = &["root", "help"];

/// What a name is for: a nameplate title shows beside its owner's name, so
/// it can't claim to be the wild either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Lineage,
    Species,
    Title,
    Account,
}

/// The name, trimmed, if it may be shown; else why not, in plain words.
pub fn check(text: &str, max: usize, kind: Kind) -> Result<String, String> {
    let t = text.trim();
    let n = t.chars().count();
    if n == 0 || n > max {
        return Err(format!("1 to {max} characters"));
    }
    if !t.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || PUNCTUATION.contains(c)) {
        return Err(format!("only letters, digits, spaces and {}", spaced(PUNCTUATION)));
    }
    if t.contains("  ") {
        return Err("no double spaces".into());
    }
    if !t.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Err("at least one letter or digit".into());
    }
    if reserved(t, kind) {
        return Err("that looks like a staff or system name".into());
    }
    Ok(t.to_string())
}

/// `.,'` as `. , '`, for messages.
fn spaced(p: &str) -> String {
    p.chars().map(String::from).collect::<Vec<_>>().join(" ")
}

/// Whether `text` would pass for the server, its staff or (a title) the
/// wild: a [`RESERVED_ANYWHERE`] fragment anywhere, or a
/// [`RESERVED_WORDS`] word, after folding.
pub fn reserved(text: &str, kind: Kind) -> bool {
    let folded = fold(text);
    if RESERVED_ANYWHERE.iter().any(|w| folded.contains(&fold(w))) {
        return true;
    }
    if reserved_whole(text) {
        return true;
    }
    let words = words(text);
    let word = |w: &str| words.iter().any(|x| *x == fold(w));
    if RESERVED_WORDS.iter().any(|w| word(w)) {
        return true;
    }
    // The wild: no title may name it; a lineage or species may not start
    // with it (it would read as a wild species: "Wild herd", playtest 6),
    // though the word may sit inside another ("Wildebeest").
    let wild = fold("wild");
    match kind {
        Kind::Title => word("wild") || word("wilds"),
        Kind::Lineage | Kind::Species => words.first().is_some_and(|w| *w == wild || *w == fold("wilds")),
        Kind::Account => false,
    }
}

/// The words of a name, folded, for the reserved-word check: split at
/// spaces and punctuation, with a run of single letters joined into one
/// word ("Head G.M." reads "head", "gm"; "M.o.d.s" reads "mods"), and each
/// CamelCase word also split into its parts ("GMBob": "gmbob", "gm",
/// "bob"; "ClaudeBot": "claudebot", "claude", "bot").
fn words(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let mut raw: Vec<String> = Vec::new();
    let mut run = String::new();
    for p in parts {
        if p.chars().count() == 1 {
            run.push_str(p);
            continue;
        }
        if !run.is_empty() {
            raw.push(std::mem::take(&mut run));
        }
        raw.push(p.to_string());
    }
    if !run.is_empty() {
        raw.push(run);
    }
    let mut out = Vec::new();
    for w in raw {
        out.push(fold(&w));
        let camel = camel_parts(&w);
        if camel.len() > 1 {
            out.extend(camel.iter().map(|c| fold(c)));
        }
    }
    out
}

/// The CamelCase parts of a word: a break before an upper-case letter that
/// follows a lower-case one or a digit, and before the last of a run of
/// capitals followed by a lower-case letter ("GMBob": "GM", "Bob").
fn camel_parts(w: &str) -> Vec<String> {
    let c: Vec<char> = w.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..c.len() {
        let lower_to_upper = (c[i - 1].is_ascii_lowercase() || c[i - 1].is_ascii_digit()) && c[i].is_ascii_uppercase();
        let acronym_end = c[i - 1].is_ascii_uppercase()
            && c[i].is_ascii_uppercase()
            && c.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
        if lower_to_upper || acronym_end {
            parts.push(c[start..i].iter().collect());
            start = i;
        }
    }
    parts.push(c[start..].iter().collect());
    parts
}

/// Whether the whole name, run together (spaces and punctuation dropped,
/// so "G.M." reads "gm"), less a leading "the", trailing digits and a team
/// word, is one of [`RESERVED_WHOLE`].
fn reserved_whole(text: &str) -> bool {
    let mut base = fold(&strip_digits(text));
    let the = fold("the");
    if base.len() > the.len() && base.starts_with(&the) {
        base = base[the.len()..].to_string();
    }
    let forms: Vec<String> = RESERVED_WHOLE.iter().map(|w| fold(w)).collect();
    if forms.contains(&base) {
        return true;
    }
    let plain: Vec<String> = PLAIN_PLURALS.iter().map(|w| fold(w)).collect();
    TEAM_SUFFIXES.iter().map(|s| fold(s)).any(|s| {
        if !(base.len() > s.len() && base.ends_with(&s)) {
            return false;
        }
        let stem = base[..base.len() - s.len()].to_string();
        forms.contains(&stem) && !(s == fold("s") && plain.contains(&stem))
    })
}

/// The letters and digits of `text` with trailing digits taken off
/// ("Herdmaster5" → "Herdmaster").
fn strip_digits(text: &str) -> String {
    let alnum: String = text.chars().filter(char::is_ascii_alphanumeric).collect();
    alnum.trim_end_matches(|c: char| c.is_ascii_digit()).to_string()
}

/// A text folded for look-alike comparison: its letters and digits only,
/// lower case, with the characters that read alike folded together (`0`
/// o, `1`/`i` l, `3` e, `4` a, `5` s, `7` t, `rn` m, `vv` w), as
/// [`crate::accounts::name_skeleton`] does for player names.
pub fn fold(text: &str) -> String {
    let alnum: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| match c {
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            c => c,
        })
        .collect();
    crate::accounts::name_skeleton(&alnum)
}

/// Whether `text` looks like one of `others` (other players' names): folded
/// alike, alike once trailing digits are dropped from both ("Herdmaster"
/// against "Herdmaster5", at least 4 characters), or, for a title,
/// containing one ("Herdmaster4 (alt)"). Names of fewer than 3 folded
/// characters only count when equal.
pub fn impersonates<'a>(text: &str, others: impl IntoIterator<Item = &'a str>, kind: Kind) -> bool {
    let f = fold(text);
    let fb = fold(&strip_digits(text));
    others.into_iter().any(|n| {
        let o = fold(n);
        let ob = fold(&strip_digits(n));
        !o.is_empty()
            && (o == f || (ob.len() >= 4 && ob == fb) || (kind == Kind::Title && o.len() >= 3 && f.contains(&o)))
    })
}

/// A name saved before the filter, made showable: characters it doesn't
/// allow (bidi overrides, zero-width and other non-ASCII characters,
/// markup) dropped, spaces collapsed, cut to `max`. `fallback` when nothing
/// showable is left or it would still pass for staff.
pub fn sanitize(text: &str, max: usize, kind: Kind, fallback: &str) -> String {
    let kept: String =
        text.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || PUNCTUATION.contains(*c)).collect();
    let words: Vec<&str> = kept.split_whitespace().collect();
    let mut s = words.join(" ");
    if s.chars().count() > max {
        s = s.chars().take(max).collect::<String>().trim_end().to_string();
    }
    match check(&s, max, kind) {
        Ok(s) => s,
        Err(_) => fallback.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_pass_for_staff_or_hide_text_are_refused() {
        for bad in [
            "rtl\u{202e}evil",
            "zero\u{200b}width",
            "Ѕerver",
            "<b>Hi</b>",
            "a &lt; b",
            "Server Admin",
            "S3rv3r ADM1N",
            "Game Master",
            "GM",
            "Mod",
            "the owner",
            "Support",
            "Sysop",
            "two  spaces",
            "--",
            "",
        ] {
            assert!(check(bad, 24, Kind::Title).is_err(), "{bad:?}");
        }
        assert!(check("wild", 24, Kind::Title).is_err());
        // Playtest 5: the whole name run together, for every kind.
        for kind in [Kind::Lineage, Kind::Species, Kind::Title, Kind::Account] {
            for bad in [
                "wild",
                "Wild",
                "W1ld",
                "the wild",
                "Wilds",
                "Wild 2",
                "G.M.",
                "G M",
                "G-M",
                "GMs",
                "M.o.d",
                "GameMod",
                "TheOwner",
                "SupportTeam",
                "DevTeam",
                "Help desk",
                "Root",
                "Anthropic",
                "Claude",
            ] {
                assert!(check(bad, 40, kind).is_err(), "{bad:?} as {kind:?}");
            }
        }
        // Ordinary names, and words merely containing a short reserved one.
        // Playtest 6: a staff word next to another word, dotted or run
        // together; misspellings; a lineage or species starting "Wild".
        for kind in [Kind::Lineage, Kind::Species, Kind::Title, Kind::Account] {
            for bad in [
                "Head G.M.",
                "herd G.M.",
                "G.M. herd",
                "Real G.M.",
                "M.o.d.s crew",
                "GMBob",
                "GMRulebender",
                "ClaudeBot",
                "ClaudeAI",
                "OwnerBob",
                "Operator",
                "Moderater",
                "Gamemastr",
                "Root user",
            ] {
                assert!(check(bad, 40, kind).is_err(), "{bad:?} as {kind:?}");
            }
        }
        for kind in [Kind::Lineage, Kind::Species] {
            assert!(check("Wild herd", 40, kind).is_err());
            assert!(check("Wilds of the north", 40, kind).is_err());
        }
        assert!(check("Herd of the wild", 40, Kind::Title).is_err());
        for ok in [
            "Wildebeest",
            "Rewilded grazers",
            "Roots",
            "Model herd",
            "Queen's grazers",
            "Steppe runners (2)",
            "Modest voles",
            "Root voles",
            "Gemstones",
            "Helpful herd",
            "Teamsters",
        ] {
            assert!(check(ok, 40, Kind::Lineage).is_ok(), "{ok:?}");
        }
        assert_eq!(check("Server Admin", 24, Kind::Lineage).unwrap_err(), "that looks like a staff or system name");
        assert_eq!(check(&"x".repeat(25), 24, Kind::Title).unwrap_err(), "1 to 24 characters");
    }

    #[test]
    fn titles_naming_another_player_are_look_alikes() {
        let others = ["Herdmaster4", "bo"];
        assert!(impersonates("Herdmaster4 (alt)", others, Kind::Title));
        assert!(impersonates("herdrnaster4", others, Kind::Lineage));
        assert!(!impersonates("Herdmaster4 (alt)", others, Kind::Lineage), "a lineage only when alike");
        assert!(!impersonates("Bold bodies", others, Kind::Title), "short names only when equal");
        assert!(impersonates("B0", others, Kind::Title));
        // A rival's name minus its digit (or with another) is a look-alike.
        assert!(impersonates("Herdmaster", others, Kind::Lineage));
        assert!(impersonates("Herdmaster7", others, Kind::Species));
        assert!(!impersonates("Bo2", ["Bo1"], Kind::Lineage), "too short to compare without digits");
    }

    #[test]
    fn old_names_are_cleaned() {
        assert_eq!(sanitize("Grazers\u{202e} of\u{200b} the  north", 40, Kind::Lineage, "x"), "Grazers of the north");
        assert_eq!(sanitize("<b>Server Admin</b>", 40, Kind::Lineage, "Lineage 3"), "Lineage 3");
        assert_eq!(sanitize("\u{202e}\u{200b}", 40, Kind::Species, "Species 2"), "Species 2");
        assert_eq!(sanitize("wild", 40, Kind::Species, "Species 7"), "Species 7");
    }
}
