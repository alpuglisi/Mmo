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

/// Folded fragments a name may not contain anywhere: long enough that no
/// common word holds them, and they would pass for the server or its staff.
/// (Shorter staff words are matched per word: [`STAFF_STEMS`].)
const RESERVED_ANYWHERE: &[&str] = &[
    "moderator",
    "administrator",
    "gamemaster",
    "gamemod",
    "sysop",
    "developer",
    "anthropic",
    // Misspellings that read the same (playtest 6).
    "moderater",
    "gamemastr",
    "rootuser",
];

/// Words a name may not use as a whole word (folded).
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
    "admin",
    "admins",
    "sysop",
    "server",
    "servers",
    "system",
    "staff",
    "official",
];

/// Staff words a word may not start or end with ("claudebot", "GMbob",
/// "botmod": playtest 7), nor hold inside after a lead-in or before a role
/// word ("realclaudebot", "theownerbob", "mygmherd": playtest 8), unless a
/// common word in [`ALLOWED_WORDS`] covers them ("modest", "landowner",
/// "cooperator"). Inside a word otherwise they pass: "dogma",
/// "badminton", "commodore".
const STAFF_STEMS: &[&str] = &[
    "gm", "gms", "mod", "mods", "admin", "admins", "owner", "owners", "claude", "support", "operator", "sysop",
    "official",
];

/// Staff words that are also parts of common words ("ecosystem",
/// "observer", "distaff", "taproot", "device", "helpful": playtest 8): only
/// refused run together with a lead-in or a role word and nothing else
/// ("serverbot", "realstaff", "rootuser", "helpteam").
const SOFT_STEMS: &[&str] = &["server", "servers", "system", "staff", "root", "dev", "devs", "help"];

/// Words that, run together before a staff word, make it a claim ("the",
/// "real", "my": "TheOwnerBob", "RealClaudeBot", "mygmherd").
const LEAD_INS: &[&str] = &[
    "the", "my", "our", "your", "ur", "real", "true", "legit", "head", "chief", "lead", "top", "im", "iam", "not",
    "ask", "official",
];

/// Words that, run together after a staff word, make it a role ("bot",
/// "team", "official").
const ROLE_WORDS: &[&str] = &[
    "bot", "bots", "team", "crew", "squad", "official", "user", "account", "ai", "hq", "desk", "admin", "help",
    "support",
];

/// Common words holding a staff word, folded: a staff word inside one of
/// these is the word, not the staff.
const ALLOWED_WORDS: &[&str] = &[
    "mode",
    "model",
    "modest",
    "modern",
    "module",
    "modular",
    "moderate",
    "modify",
    "modified",
    "modulate",
    "modicum",
    "modena",
    "modish",
    "modulo",
    "commodore",
    "gmelina",
    "paradigm",
    "phlegm",
    "diaphragm",
    "landowner",
    "homeowner",
    "shipowner",
    "downer",
    "sundowner",
    "browner",
    "drowner",
    "ownerless",
    "ownership",
    "supports",
    "supported",
    "supporting",
    "supportive",
    "supporter",
    "claudette",
    "cooperator",
    "officially",
    "unofficial",
];

/// What may follow "wild" in one word and read as the wild's own ("WildHerd",
/// "Wildlings", "Wildborn"); other words starting "wild" pass ("Wildly",
/// "Wildrose", "Wilder", "Wilde", "Wildebeest": playtest 8).
const WILD_TAILS: &[&str] = &[
    "", "s", "herd", "herds", "pack", "packs", "ling", "lings", "one", "ones", "folk", "folks", "kin", "kind", "clan",
    "clans", "tribe", "tribes", "born", "blood", "beast", "beasts", "thing", "things", "life", "team", "crew", "bot",
    "bots",
];

/// What a whole name may not come down to, once folded with its spaces and
/// punctuation dropped, a leading "the", a trailing team word and trailing
/// digits taken off (playtest 5: "G.M.", "M.o.d", "TheOwner",
/// "SupportTeam", "Help desk" and "wild" all passed): the staff words, the
/// wild (the owner of every wild animal) and the root account. ("Help" is
/// a plain word: "The Help", playtest 8.)
const RESERVED_WHOLE: &[&str] =
    &["gm", "mod", "dev", "owner", "support", "helpdesk", "claude", "wild", "root", "admin", "op", "ops"];

/// Words a staff name may end in ("SupportTeam", "DevCrew", "Mods").
const TEAM_SUFFIXES: &[&str] = &["team", "crew", "squad", "s"];

/// Whole forms that are also plain words whose plural is just a plural
/// ("Roots", "Supports"): no `s` suffix for them (playtests 6 and 8).
const PLAIN_PLURALS: &[&str] = &["root", "support"];

/// Words that only decorate a copied name ("Meadow vole II", "New
/// Sunbright Drove": playtest 8); roman numerals and digits are dropped
/// before.
const DECORATIONS: &[&str] = &[
    "new",
    "old",
    "true",
    "real",
    "original",
    "neo",
    "second",
    "third",
    "junior",
    "jr",
    "sr",
    "senior",
    "prime",
    "reborn",
    "redux",
    "returns",
    "again",
    "clone",
    "copy",
    "official",
    "nd",
    "rd",
    "th",
    "st",
    "the",
    "a",
    "an",
    // Group words: "Steppe Antelope herd", "Red fox pack" (playtest 9).
    "herd",
    "pack",
    "drove",
    "flock",
    "troop",
    "band",
    "horde",
    "clan",
    "tribe",
    "family",
    "kin",
    "line",
    "lineage",
    "bloodline",
    "colony",
    "swarm",
    "crew",
    "gang",
    "mob",
    "brood",
    "pride",
    "school",
    "group",
    "of",
];

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

/// Whether `text` would pass for the server, its staff or (not an
/// account) the wild: a [`RESERVED_ANYWHERE`] fragment anywhere, the whole
/// name a [`RESERVED_WHOLE`] one, a [`RESERVED_WORDS`] word, or a word
/// built on a staff word ([`staff_word`]), after folding.
pub fn reserved(text: &str, kind: Kind) -> bool {
    // `8` also read "at" ("Moder8or": playtest 7).
    let folded = [fold(text), fold(&text.replace('8', "at"))];
    if RESERVED_ANYWHERE.iter().any(|w| folded.iter().any(|f| f.contains(&fold(w)))) {
        return true;
    }
    if reserved_whole(text) {
        return true;
    }
    let words = words(text);
    if words.iter().any(|x| RESERVED_WORDS.iter().any(|w| *x == fold(w))) {
        return true;
    }
    if words.iter().any(|w| staff_word(w)) {
        return true;
    }
    // The wild: no title, lineage or species may use it as a word or build
    // a word on it ("Wild herd", "WildHerd", "A Wild2 herd", "TheWild":
    // playtests 6 to 8); other words starting with it pass ("Wildly").
    match kind {
        Kind::Title | Kind::Lineage | Kind::Species => words.iter().any(|w| wild_word(w)),
        Kind::Account => false,
    }
}

/// Whether one folded word is built on a staff word: a [`STAFF_STEMS`]
/// word at either end or after a lead-in or before a role word, or a
/// [`SOFT_STEMS`] word with only a lead-in or role word beside it; where no
/// [`ALLOWED_WORDS`] word covers it.
fn staff_word(w: &str) -> bool {
    let lead = |x: &str| LEAD_INS.iter().any(|l| fold(l) == x);
    let role = |x: &str| ROLE_WORDS.iter().any(|r| fold(r) == x);
    let hits = |stems: &[&str], refused: &dyn Fn(&str, &str) -> bool| {
        stems.iter().map(|s| fold(s)).any(|s| {
            w.match_indices(s.as_str()).any(|(i, _)| {
                let j = i + s.len();
                refused(&w[..i], &w[j..]) && !covered(w, i, j)
            })
        })
    };
    hits(STAFF_STEMS, &|before, after| before.is_empty() || after.is_empty() || lead(before) || role(after))
        || hits(SOFT_STEMS, &|before, after| {
            !(before.is_empty() && after.is_empty())
                && (before.is_empty() || lead(before))
                && (after.is_empty() || role(after))
        })
}

/// Whether an [`ALLOWED_WORDS`] word, or its plural, in `w` spans its
/// bytes `i..j` ("landowners" for "owners").
fn covered(w: &str, i: usize, j: usize) -> bool {
    ALLOWED_WORDS
        .iter()
        .map(|a| fold(a))
        .flat_map(|a| [format!("{a}s"), a])
        .any(|a| w.match_indices(a.as_str()).any(|(p, _)| p <= i && p + a.len() >= j))
}

/// Whether one folded word reads as the wild: "wild" and a [`WILD_TAILS`]
/// ending, or a lead-in and "wild" ("thewild", "realwild").
fn wild_word(w: &str) -> bool {
    let wild = fold("wild");
    let tail = w.strip_prefix(wild.as_str()).is_some_and(|t| WILD_TAILS.iter().any(|x| fold(x) == t));
    let led = w.strip_suffix(wild.as_str()).is_some_and(|b| LEAD_INS.iter().any(|l| fold(l) == b));
    tail || led
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
    for &p in &parts {
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
    // A single letter set apart from its word is read with it too ("C
    // laude", "Claud e": playtest 8).
    let mut joined = Vec::new();
    for (k, p) in parts.iter().enumerate() {
        if p.chars().count() != 1 {
            continue;
        }
        if let Some(next) = parts.get(k + 1).filter(|n| n.chars().count() > 1) {
            joined.push(format!("{p}{next}"));
        }
        if let Some(prev) = k.checked_sub(1).map(|i| parts[i]).filter(|n| n.chars().count() > 1) {
            joined.push(format!("{prev}{p}"));
        }
    }
    raw.extend(joined);
    let mut out = Vec::new();
    for w in raw {
        let mut forms = vec![w.clone()];
        forms.extend(camel_parts(&w).into_iter().filter(|_| camel_parts(&w).len() > 1));
        for f in forms {
            out.extend(digit_forms(&f));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// A word folded, and, when it has digits, its letters before the first
/// digit ("Claude2", "GM2", "Wild2", "Moder8or" read "claude", "gm",
/// "wild", "moder") and the word with digits at either end taken off when
/// what is left has no digit but look-alike ones ("2claude2", "2wild":
/// playtest 8): digits around a word don't hide it. Not every digit:
/// dropping all made a random id such as "mi8k3gm2" read as ending in "gm"
/// (an e2e account refused at random, after playtest 7).
fn digit_forms(w: &str) -> Vec<String> {
    let mut v = vec![fold(w)];
    if let Some(i) = w.find(|c: char| c.is_ascii_digit()) {
        if i > 0 {
            v.push(fold(&w[..i]));
        }
        let trimmed = w.trim_matches(|c: char| c.is_ascii_digit());
        if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_alphabetic() || "013457".contains(c)) {
            v.push(fold(trimmed));
        }
    }
    v
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

/// Whether `text` copies `other` (a wild species' or a rival's lineage
/// name): the same words once articles, digits, roman numerals and plurals
/// are set aside ("The Meadow voles 2"), or with only decorating words
/// added ("Meadow vole II", "Red Foxes", "New Sunbright Drove": playtest
/// 8). Names of fewer than 4 folded letters only when equal.
pub fn copies(text: &str, other: &str) -> bool {
    let o = key_words(other);
    let oc = o.concat();
    if oc.len() < 4 {
        return false;
    }
    let t = key_words(text);
    let decoration = |w: &String| DECORATIONS.iter().any(|d| fold(d) == *w);
    // Words of `text` run together to `other`'s ("MeadowVole II"), the rest
    // decorations.
    (0..t.len()).any(|i| {
        (i + 1..=t.len())
            .any(|j| t[i..j].concat() == oc && t[..i].iter().all(decoration) && t[j..].iter().all(decoration))
    })
}

/// A name's words for [`copies`]: lower case, articles, roman numerals and
/// digits dropped, folded, a plural taken off each ("foxes" "fox", "voles"
/// "vole").
fn key_words(text: &str) -> Vec<String> {
    const ROMAN: &[&str] = &["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii"];
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|w| !w.is_empty() && !["the", "a", "an"].contains(&w.as_str()) && !ROMAN.contains(&w.as_str()))
        .map(|w| fold(&w.chars().filter(|c| !c.is_ascii_digit()).collect::<String>()))
        .filter(|w| !w.is_empty())
        .map(|w| singular(&w))
        .collect()
}

/// A folded word without its plural ending ("foxes" "fox", "voles" "vole",
/// "grass" as it is) and an agent `er` read as `e` ("drovers" "drove":
/// playtest 9's "Emberwool Drovers" for "Emberwool Drove"), the same for
/// both names compared.
fn singular(w: &str) -> String {
    let es = w.strip_suffix("es").filter(|s| ["x", "s", "z", "ch", "sh"].iter().any(|e| s.ends_with(e)));
    let plural = match (es, w.strip_suffix('s')) {
        (Some(stem), _) => stem.to_string(),
        (None, Some(stem)) if stem.len() >= 3 && !stem.ends_with('s') => stem.to_string(),
        _ => w.to_string(),
    };
    match plural.strip_suffix("er") {
        Some(stem) if stem.len() >= 4 => format!("{stem}e"),
        _ => plural,
    }
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
/// against "Herdmaster5"), or every digit ("8Rulebender" against
/// "Rulebender8", playtest 8), at least 4 characters; holding one's letters
/// (at least 5, digits dropped: "The Herdmaster9", "Herdmaster9 herd",
/// "Real Herdmaster", "Herdmaster9fan": playtest 9), for every kind; for an
/// account, also one held in the other with at most 2 letters fewer; for a
/// title, containing one ("Herdmaster4 (alt)"). Names of fewer than 3
/// folded characters only count when equal.
pub fn impersonates<'a>(text: &str, others: impl IntoIterator<Item = &'a str>, kind: Kind) -> bool {
    let f = fold(text);
    let fb = fold(&strip_digits(text));
    let fl = fold(&letters(text));
    others.into_iter().any(|n| {
        let o = fold(n);
        let ob = fold(&strip_digits(n));
        let ol = fold(&letters(n));
        let (short, long) = if fl.len() <= ol.len() { (&fl, &ol) } else { (&ol, &fl) };
        !o.is_empty()
            && (o == f
                || (ob.len() >= 4 && ob == fb)
                || (ol.len() >= 4 && ol == fl)
                || (ol.len() >= 5 && fl.contains(ol.as_str()))
                || (kind == Kind::Account
                    && short.len() >= 5
                    && long.len() - short.len() <= 2
                    && long.contains(short.as_str()))
                || (kind == Kind::Title && o.len() >= 3 && f.contains(&o)))
    })
}

/// The letters of `text` (digits and the rest dropped).
fn letters(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphabetic).collect()
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

    /// Playtest 7: digits beside staff words, run-together lower case, and
    /// "wild" starting a word, without refusing ordinary names.
    #[test]
    fn staff_words_with_digits_or_run_together_are_refused_but_plain_names_pass() {
        for kind in [Kind::Lineage, Kind::Species, Kind::Title, Kind::Account] {
            for bad in [
                "Claude2 fan",
                "GM2 herd",
                "Mod2 squad",
                "Head GM2",
                "Owner2 herd",
                "Moder8or",
                "claudebot",
                "GMbob",
                "ownerbob",
                "Supportbot",
                "botmod",
            ] {
                assert!(check(bad, 40, kind).is_err(), "{bad:?} as {kind:?}");
            }
        }
        for kind in [Kind::Lineage, Kind::Species, Kind::Title] {
            for bad in ["WildHerd", "Wildherd", "A Wild herd", "Wild1 herd", "Wild2 herd", "The wildlings"] {
                assert!(check(bad, 40, kind).is_err(), "{bad:?} as {kind:?}");
            }
        }
        let benign = [
            "Meadow grazers",
            "Steppe runners",
            "Ash Fang Pack",
            "Loophole Fliers II",
            "Second Clause",
            "Thousand Hooves",
            "Brokers Four",
            "Pocket Starters",
            "Rolling Thunder Herd",
            "Queen's grazers",
            "Model herd",
            "Modest voles",
            "Modern striders",
            "Moderate grazers",
            "Paradigm runners",
            "Landowner's herd",
            "Rooted voles",
            "Rootstock",
            "Roots",
            "Root voles",
            "Supportive herd",
            "Wildebeest",
            "Wildflower moths",
            "Wilderness walkers",
            "Rewilded grazers",
            "Godwits",
            "Gemstones",
            "Helpful herd",
            "Teamsters",
            "Devon cattle",
            "Device hunters",
            "Clever devils",
            "Ownerless strays",
            "Ember foxes",
            "Owlets",
            "Moonlit hares",
            "Marsh newts",
            "Red stags",
            "Snow hares",
            "Dune skinks",
            "River otters",
            "Salt flats herd",
            "Herd 2",
            "Pack of 7",
            "Aurora",
            "Tundra wolves",
            "Golden moles",
            "Night owls",
            "Copper beetles",
            "Storm petrels",
            "Sable runners",
        ];
        for ok in benign {
            assert!(check(ok, 40, Kind::Lineage).is_ok(), "{ok:?} refused");
        }
        assert!(benign.len() >= 50);
        // Random ids with digits don't trip on a staff word inside them.
        for id in ["ctx_mi8k3gm2", "push_k3gm2x", "spec_8k3mo2", "e2e_9root4", "x2wild"] {
            assert!(check(id, 24, Kind::Account).is_ok(), "{id:?} refused");
        }
    }

    /// Playtest 8: ordinary words holding a staff word pass; staff words
    /// with a lead-in, a role word, digits around them or a letter set apart
    /// are refused.
    #[test]
    fn staff_words_are_matched_per_word_with_common_words_allowed() {
        let accepted = [
            "Taproot grazers",
            "Beetroot voles",
            "Arrowroot herd",
            "Ecosystem grazers",
            "Systematic grazers",
            "Observer pack",
            "Badminton hares",
            "Distaff herd",
            "Cooperator ants",
            "Modesty voles",
            "Modena runners",
            "Claudette herd",
            "Wildly happy herd",
            "Wildrose grazers",
            "Wilder herd",
            "Oscar Wilde voles",
            "Supports",
            "The Help",
            "Gmelina moths",
            "Staffpick",
            // Others of the same kinds.
            "Dogma runners",
            "Pigment moths",
            "Commodore herd",
            "Sundowner foxes",
            "Helpful herd",
            "Devon cattle",
            "Root grazers",
            "RootGrazers",
            "Wildebeest",
        ];
        for kind in [Kind::Lineage, Kind::Species, Kind::Title] {
            for ok in accepted {
                assert!(check(ok, 40, kind).is_ok(), "{ok:?} as {kind:?} refused");
            }
        }
        let refused_everywhere = [
            "realclaudebot",
            "theownerbob",
            "mygmherd",
            "2claude2 fan",
            "C laude fan",
            "Claud e fan",
            "serverbot",
            "realstaff",
            "rootuser",
            "adminbob",
            "Badmin",
            "officialbob",
            "TheModsTeam",
            "Supportbot",
            "Mod2 squad",
        ];
        let refused_names = ["2wild herd", "TheWild", "Wildherd", "Wildlings", "Wild herd"];
        for kind in [Kind::Lineage, Kind::Species, Kind::Title, Kind::Account] {
            for bad in refused_everywhere {
                assert!(check(bad, 40, kind).is_err(), "{bad:?} as {kind:?} passed");
            }
        }
        for kind in [Kind::Lineage, Kind::Species, Kind::Title] {
            for bad in refused_names {
                assert!(check(bad, 40, kind).is_err(), "{bad:?} as {kind:?} passed");
            }
        }
    }

    /// Playtest 8: copies of a wild or rival name with decorations added.
    #[test]
    fn copies_with_decorations_are_copies() {
        let cases: &[(&str, &str, bool)] = &[
            ("Meadow vole", "Meadow vole", true),
            ("The Meadow voles 2", "Meadow vole", true),
            ("Meadow vole II", "Meadow vole", true),
            ("MeadowVole III", "Meadow vole", true),
            ("New Meadow vole", "Meadow vole", true),
            ("Red Foxes", "Red fox", true),
            ("Sunbright Drove II", "Sunbright Drove", true),
            ("New Sunbright Drove", "Sunbright Drove", true),
            ("Real Sunbright Drove 2", "Sunbright Drove", true),
            ("Meadow vole hunters", "Meadow vole", false),
            // Playtest 9: near copies.
            ("Ironjaws", "Ironjaw", true),
            ("Ironjaw", "Ironjaws", true),
            ("Emberwool Drovers", "Emberwool Drove", true),
            ("Steppe Antelope herd", "Steppe antelope", true),
            ("Red fox pack", "Red fox", true),
            ("Pack of the Red fox", "Red fox", true),
            ("Sunbright", "Sunbright Drove", false),
            ("Red foxglove", "Red fox", false),
            ("Grass snakes", "Grass", false),
            ("L2", "L1", false),
        ];
        for &(text, other, copy) in cases {
            assert_eq!(copies(text, other), copy, "{text:?} against {other:?}");
        }
    }

    #[test]
    fn titles_naming_another_player_are_look_alikes() {
        let others = ["Herdmaster4", "bo"];
        assert!(impersonates("Herdmaster4 (alt)", others, Kind::Title));
        assert!(impersonates("herdrnaster4", others, Kind::Lineage));
        assert!(
            impersonates("Herdmaster4 (alt)", others, Kind::Lineage),
            "holding a name, for every kind (playtest 9)"
        );
        assert!(!impersonates("Bold bodies", others, Kind::Title), "short names only when equal");
        assert!(impersonates("B0", others, Kind::Title));
        // A rival's name minus its digit (or with another) is a look-alike.
        assert!(impersonates("Herdmaster", others, Kind::Lineage));
        assert!(impersonates("Herdmaster7", others, Kind::Species));
        assert!(!impersonates("Bo2", ["Bo1"], Kind::Lineage), "too short to compare without digits");
        // Playtest 8: digits moved or a letter or two added.
        let cases: &[(&str, &str, Kind, bool)] = &[
            ("8Rulebender", "Rulebender8", Kind::Account, true),
            ("8Rulebender", "Rulebender8", Kind::Lineage, true),
            ("Herdmaster8x", "Herdmaster8", Kind::Account, true),
            ("xHerdmaster", "Herdmaster8", Kind::Account, true),
            ("Herdmaster8x", "Herdmaster8", Kind::Lineage, true),
            ("Herdmasterxyz", "Herdmaster8", Kind::Account, true),
            ("usera", "userb", Kind::Account, false),
            // Holding a player's name, for every kind (playtest 9).
            ("Grazers", "Grazer", Kind::Lineage, true),
            ("Herdmaster9x", "Herdmaster9", Kind::Lineage, true),
            ("xHerdmaster9", "Herdmaster9", Kind::Lineage, true),
            ("The Herdmaster9", "Herdmaster9", Kind::Lineage, true),
            ("Herdmaster9 herd", "Herdmaster9", Kind::Lineage, true),
            ("Herdmaster9's Drove", "Herdmaster9", Kind::Lineage, true),
            ("Herdmaster9 II", "Herdmaster9", Kind::Species, true),
            ("Real Herdmaster", "Herdmaster9", Kind::Title, true),
            ("TheHerdmaster9", "Herdmaster9", Kind::Account, true),
            ("Herdmaster9fan", "Herdmaster9", Kind::Account, true),
            ("Steppe grazers", "Bob", Kind::Lineage, false),
            ("Bobcats", "Bobby", Kind::Lineage, false),
        ];
        for &(text, other, kind, alike) in cases {
            assert_eq!(impersonates(text, [other], kind), alike, "{text:?} against {other:?} as {kind:?}");
        }
    }

    #[test]
    fn old_names_are_cleaned() {
        assert_eq!(sanitize("Grazers\u{202e} of\u{200b} the  north", 40, Kind::Lineage, "x"), "Grazers of the north");
        assert_eq!(sanitize("<b>Server Admin</b>", 40, Kind::Lineage, "Lineage 3"), "Lineage 3");
        assert_eq!(sanitize("\u{202e}\u{200b}", 40, Kind::Species, "Species 2"), "Species 2");
        assert_eq!(sanitize("wild", 40, Kind::Species, "Species 7"), "Species 7");
    }
}
