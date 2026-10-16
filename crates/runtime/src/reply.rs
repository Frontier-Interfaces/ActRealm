//! Whether an Agent's final reply hands the turn back to the user with a
//! question or an explicit request to answer.
//!
//! The check is conservative and runs on the message as the Provider reported
//! it (Claude/Codex `Stop.last_assistant_message`, Codex `last_agent_message`).
//! Only the last paragraph of prose counts, and mostly its last sentence:
//!
//! 1. Fenced code blocks (backticks or tildes; an unclosed fence runs to the
//!    end), headings, horizontal rules, table rows and block quotes are
//!    dropped, and each of them ends the paragraph before it.
//! 2. In the remaining lines, list markers, task boxes, emphasis markers,
//!    inline code spans, bare URLs and link targets are removed (a link keeps
//!    its text). A question inside backticks therefore never counts.
//! 3. The last sentence starts after the paragraph's last line break or
//!    sentence terminator (`.`, `!`, `?` or `;` followed by whitespace, any
//!    full-width terminator, an ellipsis).
//!
//! That sentence awaits a reply when
//!
//! - it ends with `?` or a full-width question mark (closing quotes,
//!   brackets, emphasis or emoji after the mark are ignored);
//! - it ends with the Chinese sentence-final particle `ma` or `ne`;
//! - it contains one of [`CHINESE_REQUESTS`] in an allowed position;
//! - it contains one of [`ENGLISH_REQUESTS`], case-insensitive and bounded by
//!   non-letters on both sides.
//!
//! Request phrases are looked for in the last 2,048 bytes of the sentence.
//! Quoted text (`「…」`, `『…』`, curly or straight double quotes) is a
//! mention, not a request, and is left out.
//!
//! An earlier sentence of the same paragraph awaits a reply too when it is a
//! question put to the user ("should we change it like this? there is also a
//! more cautious way ..."): it ends with a question mark and ends with a
//! question particle, addresses the user or names a choice, or holds an
//! English request.
//!
//! Only a turn that explicitly waits for the user's answer awaits a reply
//! (founder, 2026-10-16). An offer of more help completes the turn, whether
//! or not it asks to be told: a conditional offer ("if you also think it is
//! useless, I will delete it too", "if it still does not feel smooth, tell
//! me", "say the word if you want it committed", "let me know if ...") and a
//! "that is all it takes" offer ("to change the color, just tell me") read as
//! completed. Tell me, say the word, want me to and let me know are offers in
//! such a sentence; every other request, and every question, still counts.
//! Questions and requests in earlier paragraphs never count.

use std::collections::HashMap;

/// Chinese requests to the user and where in the sentence they count.
///
/// The phrases, in order: whether or not (yao bu yao); whether (shi fou); you
/// choose (two forms of "you"); choose which; tell me; say the word; confirm
/// it (que ren yi xia); you decide (two forms); want me to (yao wo); is that
/// all right (ke yi ma, hao ma, xing ma).
const CHINESE_REQUESTS: &[(&str, Placement)] = &[
    ("\u{8981}\u{4e0d}\u{8981}", Placement::Anywhere),
    ("\u{662f}\u{5426}", Placement::ClauseStartOrAddressed),
    ("\u{4f60}\u{9009}", Placement::NotBefore(PAST_CHOICE)),
    ("\u{60a8}\u{9009}", Placement::NotBefore(PAST_CHOICE)),
    ("\u{9009}\u{54ea}", Placement::Anywhere),
    ("\u{544a}\u{8bc9}\u{6211}", Placement::NotBefore(PAST)),
    ("\u{8bf4}\u{4e00}\u{58f0}", Placement::Anywhere),
    (
        "\u{786e}\u{8ba4}\u{4e00}\u{4e0b}",
        Placement::NotFirstPerson,
    ),
    ("\u{4f60}\u{51b3}\u{5b9a}", Placement::NotBefore(PAST)),
    ("\u{60a8}\u{51b3}\u{5b9a}", Placement::NotBefore(PAST)),
    ("\u{8981}\u{6211}", Placement::ClauseStart),
    ("\u{53ef}\u{4ee5}\u{5417}", Placement::Anywhere),
    ("\u{597d}\u{5417}", Placement::Anywhere),
    ("\u{884c}\u{5417}", Placement::Anywhere),
];

/// Requests that only offer more help when the sentence makes them depend on
/// something: tell me, say the word, want me to.
const CHINESE_OFFERS: &[&str] = &[
    "\u{544a}\u{8bc9}\u{6211}",
    "\u{8bf4}\u{4e00}\u{58f0}",
    "\u{8981}\u{6211}",
];

/// What makes an offer conditional: if (ru guo, yao shi, jia ru, wan yi,
/// tang ruo, ruo shi), "... if so" (de hua), once (yi dan), if there is a
/// problem (you wen ti, chu wen ti), if needed (you xu yao, xu yao shi).
const CHINESE_CONDITIONS: &[&str] = &[
    "\u{5982}\u{679c}",
    "\u{8981}\u{662f}",
    "\u{5047}\u{5982}",
    "\u{4e07}\u{4e00}",
    "\u{5018}\u{82e5}",
    "\u{82e5}\u{662f}",
    "\u{7684}\u{8bdd}",
    "\u{4e00}\u{65e6}",
    "\u{6709}\u{95ee}\u{9898}",
    "\u{51fa}\u{95ee}\u{9898}",
    "\u{6709}\u{9700}\u{8981}",
    "\u{9700}\u{8981}\u{65f6}",
];

/// "That is all it takes" after an offer: jiu xing, ji ke, jiu hao.
const CHINESE_ENOUGH: &[&str] = &["\u{5c31}\u{884c}", "\u{5373}\u{53ef}", "\u{5c31}\u{597d}"];

/// What marks an earlier sentence ending in a question mark as a question to
/// the user: you (two forms), whether or not (yao bu yao, shi bu shi, shi
/// fou), want me to, which one (na ge, na zhong, na yi).
const CHINESE_ASKING: &[&str] = &[
    "\u{4f60}",
    "\u{60a8}",
    "\u{8981}\u{4e0d}\u{8981}",
    "\u{662f}\u{4e0d}\u{662f}",
    "\u{662f}\u{5426}",
    "\u{8981}\u{6211}",
    "\u{54ea}\u{4e2a}",
    "\u{54ea}\u{79cd}",
    "\u{54ea}\u{4e00}",
];

/// Question particles: ma, me, ne, ba.
const QUESTION_PARTICLES: [char; 4] = ['\u{5417}', '\u{4e48}', '\u{5462}', '\u{5427}'];

/// Quotation marks, opening and closing: corner brackets, white corner
/// brackets, curly double quotes, straight double quotes.
const QUOTES: [(char, char); 4] = [
    ('\u{300c}', '\u{300d}'),
    ('\u{300e}', '\u{300f}'),
    ('\u{201c}', '\u{201d}'),
    ('"', '"'),
];

/// English requests to the user. A bare "which" is not one: it is far more
/// often a relative pronoun ("fixed the parser, which was dropping lines").
const ENGLISH_REQUESTS: &[&str] = &[
    "should i",
    "shall i",
    "do you want",
    "would you like",
    "would you prefer",
    "do you prefer",
    "let me know",
    "please confirm",
    "can you",
    "could you",
    "which one",
    "which ones",
    "which option",
    "which options",
    "which approach",
    "which do you",
    "which would you",
];

/// English requests that only offer more help in a conditional sentence.
const ENGLISH_OFFERS: &[&str] = &["let me know"];

/// What makes an English offer conditional.
const ENGLISH_CONDITIONS: &[&str] = &["if", "in case", "whenever"];

/// Particles after a phrase that turn it into a report about the past: de
/// (the one you ...), le (done), guo (already).
const PAST: &[&str] = &["\u{7684}", "\u{4e86}", "\u{8fc7}"];

/// `PAST`, plus the same particles after the longer verbs "select", "pick
/// out" and "settle on".
const PAST_CHOICE: &[&str] = &[
    "\u{7684}",
    "\u{4e86}",
    "\u{8fc7}",
    "\u{62e9}\u{7684}",
    "\u{62e9}\u{4e86}",
    "\u{62e9}\u{8fc7}",
    "\u{4e2d}\u{7684}",
    "\u{4e2d}\u{4e86}",
    "\u{5b9a}\u{7684}",
    "\u{5b9a}\u{4e86}",
];

/// Clause openings that address the user: you (two forms), please, would you
/// mind, help me.
const ADDRESSING: &[&str] = &[
    "\u{4f60}",
    "\u{60a8}",
    "\u{8bf7}",
    "\u{9ebb}\u{70e6}",
    "\u{5e2e}\u{6211}",
];

/// Bytes at the end of the last sentence searched for a request phrase.
const MAX_REQUEST_SCAN: usize = 2048;

/// The first-person pronoun wo.
const FIRST_PERSON: char = '\u{6211}';

/// The sentence-final particles ma and ne.
const FINAL_PARTICLES: [char; 2] = ['\u{5417}', '\u{5462}'];

#[derive(Debug, Clone, Copy)]
enum Placement {
    /// Anywhere in the sentence.
    Anywhere,
    /// Unless one of these follows directly ("the plan you chose", "what you
    /// told me").
    NotBefore(&'static [&'static str]),
    /// Only at the start of a clause, so "need me to" and "do not need me"
    /// do not count.
    ClauseStart,
    /// At the start of a clause, or in a clause that opens by addressing the
    /// user ("please confirm whether ..."), so "I checked whether ..." does
    /// not count.
    ClauseStartOrAddressed,
    /// Unless the clause has the Agent speaking about itself ("I will
    /// double-check") without addressing the user.
    NotFirstPerson,
}

impl Placement {
    /// `cut` is true when `sentence` is only the end of a longer sentence.
    fn allows(self, sentence: &str, start: usize, end: usize, cut: bool) -> bool {
        let clause = || clause_before(sentence, start, cut);
        match self {
            Self::Anywhere => true,
            Self::NotBefore(endings) => {
                let after = &sentence[end..];
                !endings.iter().any(|ending| after.starts_with(ending))
            }
            Self::ClauseStart => clause().is_some_and(str::is_empty),
            Self::ClauseStartOrAddressed => {
                clause().is_some_and(|clause| clause.is_empty() || addresses_user(clause))
            }
            Self::NotFirstPerson => clause()
                .is_some_and(|clause| !clause.contains(FIRST_PERSON) || addresses_user(clause)),
        }
    }
}

/// The clause text before `start`, or None when the clause began before a
/// `cut` sentence and its start is unknown.
fn clause_before(sentence: &str, start: usize, cut: bool) -> Option<&str> {
    let before = &sentence[..start];
    match before
        .char_indices()
        .rev()
        .find(|(_, c)| is_clause_separator(*c))
    {
        Some((offset, separator)) => Some(before[offset + separator.len_utf8()..].trim()),
        None => (!cut).then(|| before.trim()),
    }
}

/// True when the reply's last sentence asks the user something or asks the
/// user to answer. See the module documentation for the exact rule.
pub fn message_awaits_reply(message: &str) -> bool {
    last_prose_line(message).is_some_and(|line| {
        let start = last_sentence_start(&line);
        sentence_awaits_reply(line[start..].trim())
            || sentences(&line[..start]).into_iter().any(asks_user)
    })
}

/// An earlier sentence of the last paragraph that puts a question to the
/// user; see the module documentation.
fn asks_user(sentence: &str) -> bool {
    let sentence = without_quotes(sentence);
    let trimmed = sentence.trim().trim_end_matches(is_trailing_noise);
    if !trimmed.ends_with(['?', '\u{ff1f}']) {
        return false;
    }
    let core = trimmed.trim_end_matches(|c| is_terminator(c) || is_trailing_noise(c));
    core.ends_with(QUESTION_PARTICLES)
        || CHINESE_ASKING.iter().any(|word| core.contains(word))
        || english_request(core)
}

/// The sentences of `text`, each with its terminator.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((offset, c)) = characters.next() {
        if is_sentence_boundary(c, characters.peek().map(|(_, next)| *next)) {
            let end = offset + c.len_utf8();
            if !text[start..end].trim().is_empty() {
                out.push(&text[start..end]);
            }
            start = end;
        }
    }
    if !text[start..].trim().is_empty() {
        out.push(&text[start..]);
    }
    out
}

/// `text` with quoted spans blanked out. An opening mark without its closing
/// mark is kept as it is.
fn without_quotes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(c) = text[index..].chars().next() {
        let close = QUOTES
            .iter()
            .find(|(open, _)| *open == c)
            .map(|(_, close)| *close);
        let after = index + c.len_utf8();
        if let Some(end) = close.and_then(|close| text[after..].find(close).map(|at| after + at)) {
            out.push(' ');
            index = end + close.map_or(0, char::len_utf8);
            continue;
        }
        out.push(c);
        index = after;
    }
    out
}

fn sentence_awaits_reply(sentence: &str) -> bool {
    let sentence = without_quotes(sentence);
    let trimmed = sentence.trim_end_matches(is_trailing_noise);
    let core = trimmed.trim_end_matches(|c| is_terminator(c) || is_trailing_noise(c));
    if trimmed[core.len()..].contains(['?', '\u{ff1f}']) {
        return true;
    }
    if core.ends_with(FINAL_PARTICLES) {
        return true;
    }
    // A closing request is short. Looking only at the end of an unusually
    // long sentence keeps the clause checks below linear.
    let mut start = core.len().saturating_sub(MAX_REQUEST_SCAN);
    while !core.is_char_boundary(start) {
        start += 1;
    }
    let tail = &core[start..];
    chinese_request(tail, start > 0) || english_request(tail)
}

fn chinese_request(sentence: &str, cut: bool) -> bool {
    let conditional = CHINESE_CONDITIONS
        .iter()
        .any(|word| sentence.contains(word));
    CHINESE_REQUESTS.iter().any(|(phrase, placement)| {
        sentence.match_indices(phrase).any(|(start, _)| {
            let end = start + phrase.len();
            let offer = CHINESE_OFFERS.contains(phrase)
                && (conditional
                    || CHINESE_ENOUGH
                        .iter()
                        .any(|word| sentence[end..].contains(word)));
            !offer && placement.allows(sentence, start, end, cut)
        })
    })
}

fn english_request(sentence: &str) -> bool {
    let mut text = String::with_capacity(sentence.len());
    for word in sentence.split_whitespace() {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&word.to_ascii_lowercase());
    }
    let conditional = ENGLISH_CONDITIONS
        .iter()
        .any(|word| contains_word(&text, word));
    ENGLISH_REQUESTS.iter().any(|phrase| {
        !(conditional && ENGLISH_OFFERS.contains(phrase)) && contains_word(&text, phrase)
    })
}

/// `phrase` occurs in `text` bounded by non-letters on both sides.
fn contains_word(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + phrase.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric())
            && !after.is_some_and(|c| c.is_ascii_alphanumeric())
    })
}

fn addresses_user(clause: &str) -> bool {
    ADDRESSING.iter().any(|word| clause.starts_with(word))
}

fn is_clause_separator(c: char) -> bool {
    matches!(
        c,
        ',' | ':' | '(' | '\u{ff0c}' | '\u{3001}' | '\u{ff1a}' | '\u{ff08}'
    )
}

fn is_terminator(c: char) -> bool {
    matches!(
        c,
        '.' | '!'
            | '?'
            | ';'
            | '\u{3002}'
            | '\u{ff01}'
            | '\u{ff1f}'
            | '\u{ff1b}'
            | '\u{ff0e}'
            | '\u{2026}'
    )
}

/// Whitespace and closing decoration after a sentence: quotes, brackets,
/// emphasis markers, emoji. Letters, digits and terminators are kept.
fn is_trailing_noise(c: char) -> bool {
    c.is_whitespace() || (!c.is_alphanumeric() && !is_terminator(c))
}

/// Where the paragraph's last sentence starts: after its last line break or
/// sentence terminator, the terminators that end the paragraph aside.
fn last_sentence_start(paragraph: &str) -> usize {
    let text = paragraph.trim_end_matches(is_trailing_noise);
    let body = text.trim_end_matches(|c| is_terminator(c) || is_trailing_noise(c));
    let mut start = 0;
    let mut characters = body.char_indices().peekable();
    while let Some((offset, c)) = characters.next() {
        if is_sentence_boundary(c, characters.peek().map(|(_, next)| *next)) {
            start = offset + c.len_utf8();
        }
    }
    start
}

/// A line break, an ASCII terminator followed by whitespace, or any
/// full-width terminator or ellipsis.
fn is_sentence_boundary(c: char, next: Option<char>) -> bool {
    match c {
        '\n' => true,
        '.' | '!' | '?' | ';' => next.is_some_and(char::is_whitespace),
        _ => is_terminator(c),
    }
}

/// The last line of prose, cleaned up. A line break ends a sentence, so the
/// last sentence of the last prose paragraph lies on this line.
fn last_prose_line(message: &str) -> Option<String> {
    let mut fence: Option<(char, usize)> = None;
    let mut candidates = Vec::new();
    for line in message.lines() {
        let line = line.trim();
        if let Some(open) = fence {
            if closes_fence(line, open) {
                fence = None;
            }
        } else if let Some(open) = opens_fence(line) {
            fence = Some(open);
        } else if !is_structural(line) {
            candidates.push(line);
        }
    }
    candidates
        .into_iter()
        .rev()
        .map(|line| inline_text(strip_list_marker(line)))
        .find(|text| !text.is_empty())
}

fn opens_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let length = line.chars().take_while(|c| *c == marker).count();
    // A backtick "fence" whose rest holds another backtick is inline code.
    (length >= 3 && !(marker == '`' && line[length..].contains('`'))).then_some((marker, length))
}

fn closes_fence(line: &str, (marker, length): (char, usize)) -> bool {
    let run = line.chars().take_while(|c| *c == marker).count();
    run >= length && line[run..].trim().is_empty()
}

fn is_structural(line: &str) -> bool {
    if line.is_empty() || line.starts_with(['>', '|']) {
        return true;
    }
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes)
        && line[hashes..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
    {
        return true;
    }
    let mut marks = line.chars().filter(|c| !c.is_whitespace());
    marks.next().is_some_and(|first| {
        matches!(first, '-' | '*' | '_') && marks.clone().all(|c| c == first) && marks.count() >= 2
    })
}

/// Text without emphasis markers, inline code spans, URLs or link targets (a
/// link keeps its text). Runs in linear time on any input.
fn inline_text(text: &str) -> String {
    let spans = code_spans(text);
    let mut spans = spans.iter().peekable();
    let mut links = Links::new(text);
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(c) = text[index..].chars().next() {
        while spans.peek().is_some_and(|(start, _)| *start < index) {
            spans.next();
        }
        let rest = &text[index..];
        if let Some(&&(start, end)) = spans.peek() {
            if start == index {
                out.push(' ');
                index = end;
                continue;
            }
        }
        if let Some(length) = url_length(rest) {
            out.push(' ');
            index += length;
            continue;
        }
        let open = index + usize::from(c == '!');
        if let Some((label, end)) = links.at(open) {
            out.push_str(&inline_text(label));
            index = end;
            continue;
        }
        if rest.starts_with("~~") {
            index += 2;
            continue;
        }
        if !matches!(c, '*' | '`') {
            out.push(c);
        }
        index += c.len_utf8();
    }
    out.trim().to_owned()
}

fn strip_list_marker(line: &str) -> &str {
    let rest = if let Some(rest) = line.strip_prefix(['-', '*', '+', '\u{2022}']) {
        rest
    } else {
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 || digits > 9 {
            return line;
        }
        match line[digits..].strip_prefix(['.', ')']) {
            Some(rest) => rest,
            None => return line,
        }
    };
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return line;
    }
    let rest = rest.trim_start();
    ["[ ]", "[x]", "[X]"]
        .iter()
        .find_map(|task| rest.strip_prefix(task))
        .map_or(rest, str::trim_start)
}

/// Byte ranges of inline code spans: a run of backticks up to the next run
/// of the same length. Unmatched backticks open nothing.
fn code_spans(text: &str) -> Vec<(usize, usize)> {
    // Backticks are single bytes and never part of a multi-byte character.
    let bytes = text.as_bytes();
    let mut runs = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'`' {
            let start = index;
            while bytes.get(index) == Some(&b'`') {
                index += 1;
            }
            runs.push((start, index - start));
        } else {
            index += 1;
        }
    }
    let mut next_same = vec![None; runs.len()];
    let mut seen = HashMap::new();
    for (position, (_, length)) in runs.iter().enumerate().rev() {
        next_same[position] = seen.insert(*length, position);
    }
    let mut spans = Vec::new();
    let mut position = 0;
    while position < runs.len() {
        match next_same[position] {
            Some(close) => {
                let (end, length) = runs[close];
                spans.push((runs[position].0, end + length));
                position = close + 1;
            }
            None => position += 1,
        }
    }
    spans
}

/// The length of a bare or angle-bracketed URL at the start of `text`.
fn url_length(text: &str) -> Option<usize> {
    let bracketed = usize::from(text.starts_with('<'));
    let url = &text[bracketed..];
    let scheme = ["http://", "https://"].iter().any(|scheme| {
        url.as_bytes()
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme.as_bytes()))
    });
    if !scheme {
        return None;
    }
    let end = url
        .find(|c: char| c.is_whitespace() || c == '>')
        .unwrap_or(url.len());
    let closing = usize::from(bracketed == 1 && url[end..].starts_with('>'));
    Some(bracketed + end + closing)
}

/// Finds Markdown links `[label](target)` while a line is read from left to
/// right. The next closing bracket and parenthesis are remembered, so a line
/// full of brackets is still read in linear time.
struct Links<'a> {
    text: &'a str,
    bracket: NextByte,
    parenthesis: NextByte,
}

impl<'a> Links<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            bracket: NextByte::new(b']'),
            parenthesis: NextByte::new(b')'),
        }
    }

    /// The label of a link that opens at `open`, and the offset after it.
    fn at(&mut self, open: usize) -> Option<(&'a str, usize)> {
        if !self.text[open..].starts_with('[') {
            return None;
        }
        let close = self.bracket.at_or_after(self.text, open + 1)?;
        if !self.text[close + 1..].starts_with('(') {
            return None;
        }
        let end = self.parenthesis.at_or_after(self.text, close + 2)?;
        Some((&self.text[open + 1..close], end + 1))
    }
}

/// The next position of one ASCII byte, for positions that never decrease.
struct NextByte {
    byte: u8,
    found: Option<usize>,
    searched_to: usize,
}

impl NextByte {
    fn new(byte: u8) -> Self {
        Self {
            byte,
            found: None,
            searched_to: 0,
        }
    }

    fn at_or_after(&mut self, text: &str, position: usize) -> Option<usize> {
        if let Some(found) = self.found.filter(|found| *found >= position) {
            return Some(found);
        }
        let from = position.max(self.searched_to);
        self.found = text.as_bytes()[from.min(text.len())..]
            .iter()
            .position(|byte| *byte == self.byte)
            .map(|offset| from + offset);
        self.searched_to = self.found.map_or(text.len(), |found| found + 1);
        self.found
    }
}

#[cfg(test)]
mod tests {
    use super::message_awaits_reply;

    fn check(cases: &[(&str, bool)]) {
        for (message, expected) in cases {
            assert_eq!(
                message_awaits_reply(message),
                *expected,
                "message: {message:?}"
            );
        }
    }

    #[test]
    fn chinese_questions_and_requests_await_a_reply() {
        check(&[
            ("测试全部通过。要不要我顺便提交", true),
            ("两个方案都能用，你选一个。", true),
            ("A 和 B 选哪个", true),
            ("哪个方案更合适，告诉我。", true),
            ("标题栏还空着，告诉我填什么。", true),
            ("还有一处没推，需要你决定。", true),
            ("请帮我确认一下", true),
            ("确认一下再合并。", true),
            ("你决定吧。", true),
            ("要我继续把文档也改了", true),
            ("这样改可以吗。", true),
            ("先这样，好吗", true),
            ("可以合并了吗", true),
            ("你觉得呢", true),
            ("请确认是否合并。", true),
            ("是否需要继续", true),
            ("你看是否合适", true),
            ("已改好，要提交吗？", true),
            ("**要不要我顺便提交？**", true),
            ("已经改完了（要推送吗？）", true),
            ("要提交吗？🙂", true),
            ("- 修好了 A\n- 补了测试\n\n要提交吗", true),
            ("如果没问题，要我提交吗？", true),
            // A question put to the user before the closing sentence.
            (
                "要照这个改吗？还有一个更保守的做法：只把「出问题再告诉我」这类算成做完。",
                true,
            ),
            ("你打算接什么设备？把型号发给我。", true),
        ]);
    }

    #[test]
    fn chinese_reports_do_not_await_a_reply() {
        check(&[
            ("已完成，测试全部通过。", false),
            // A conditional offer without an explicit request: the Agent
            // reports what it did and what it would do; the turn reads as
            // completed.
            ("你要是也觉得没用，我一并删掉。", false),
            ("如果需要，我可以再加测试。", false),
            ("我检查了是否有遗漏，没有发现问题。", false),
            ("我帮你检查了是否有遗漏。", false),
            ("已按你选的方案 B 完成。", false),
            ("你选择的布局已经生效。", false),
            ("你告诉我的路径已经修好。", false),
            ("按你决定的顺序处理了。", false),
            ("我再确认一下 CI 结果。", false),
            ("这一步不需要我手动处理。", false),
            ("要不要我提交？\n\n已经提交并推送了。", false),
            ("为什么会失败？因为缓存过期了。", false),
            // An offer of more help completes the turn (founder, 2026-10-16).
            (
                "不用再做别的。你试双指左滑时，如果滑开后一两秒自己收回去，或者还是觉得不顺，告诉我。",
                false,
            ),
            ("要提交的话说一声。", false),
            ("需要的话告诉我。", false),
            ("你想修的话，告诉我一声，我先安排实测。", false),
            ("关了的话，告诉我一声「继续」就行。", false),
            ("要改灯光颜色，告诉我就行。", false),
            ("有问题告诉我。", false),
            ("要我继续的话说一声。", false),
            // Quoted text is a mention.
            ("我把「要不要」改成了「是否」。", false),
            ("提示文案改成了“要我继续吗”。", false),
        ]);
    }

    #[test]
    fn english_questions_and_requests_await_a_reply() {
        check(&[
            ("Should I push?", true),
            ("All green. Should I push", true),
            ("Do you want me to open a PR.", true),
            ("Would you like me to update the docs", true),
            ("Let me know which one you prefer.", true),
            ("Should I push it now? The branch is ready.", true),
            ("Please confirm the version bump.", true),
            ("Which one do you prefer.", true),
            ("SHALL I MERGE IT", true),
            (
                "Done.\n\n- updated A\n- updated B\n\nWhich option should we keep",
                true,
            ),
            ("Should I push?\n\n```\ncargo test\n```", true),
            ("Should I run `cargo test`?", true),
            ("Can you check it on the device", true),
        ]);
    }

    #[test]
    fn english_reports_do_not_await_a_reply() {
        check(&[
            ("Done. The build is green.", false),
            ("Why did it fail? The cache was stale.", false),
            ("I fixed the parser, which was dropping lines.", false),
            ("If you want, I can also add tests.", false),
            ("Let me know if you want the old file removed.", false),
            ("If anything breaks, let me know.", false),
            ("Should I push?\n\nNever mind, I pushed it.", false),
            ("I renamed `isReady?` to `ready`.", false),
            ("The regex is `^a?$`", false),
            ("See https://example.com/search?q=1", false),
            ("See [the report](https://example.com/report?id=1)", false),
            ("Let me knowledge-check this later.", false),
            ("Tests pass; I should investigate the flake later.", false),
        ]);
    }

    #[test]
    fn code_headings_quotes_and_empty_text_do_not_await_a_reply() {
        check(&[
            ("```\nfoo?\n```", false),
            ("~~~\n要提交吗？\n~~~", false),
            ("```foo?```", false),
            ("Done.\n```text\nShould I push?", false),
            ("All tests pass.\n\n```\nShould I push?\n```", false),
            ("## What changed?\n\nAll tests pass.", false),
            ("All tests pass.\n\n## Next steps?", false),
            ("> Should I push?", false),
            ("Done.\n\n> 要不要提交？", false),
            ("| Ready? | yes |", false),
            ("Done.\n\n---", false),
            ("", false),
            ("   \n\n  ", false),
        ]);
    }

    #[test]
    fn paragraph_before_a_trailing_code_block_or_rule_is_the_last_prose() {
        check(&[
            ("要不要我提交？\n\n```\ngit status\n```", true),
            ("Should I push?\n\n---", true),
            ("Should I push?\n\n> note: the CI is slow", true),
            (
                "What does this print?\n```\necho hi\n```\nIt prints hi.",
                false,
            ),
        ]);
    }

    #[test]
    fn links_lists_and_task_boxes_keep_only_their_text() {
        check(&[
            ("Done, see [`ready?`](https://example.com).", false),
            ("Open [the PR](https://example.com/pr?id=1)?", true),
            ("1. Fixed A\n2. [ ] Should I also fix B", true),
            ("* * *\n\n- [x] all done", false),
            ("<https://example.com/q?>", false),
        ]);
    }

    #[test]
    fn hostile_lines_are_processed_in_linear_time() {
        let brackets = "[".repeat(200_000) + "](";
        let backticks = (1..2_000)
            .map(|n| "`".repeat(n % 7 + 1) + "a")
            .collect::<String>();
        let urls = "http://a ".repeat(50_000);
        let fences = "```\n".repeat(50_001) + "Should I push?";
        let markers = "我检查了".to_owned() + &"是否".repeat(100_000);
        for message in [brackets, backticks, urls, markers] {
            assert!(!message_awaits_reply(&message));
        }
        assert!(!message_awaits_reply(&fences));
    }
}
