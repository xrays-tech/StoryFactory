// src/services/prose_lint.rs — 正文“去 AI 味”静态检测（纯函数，无 IO）
use std::collections::HashMap;

const SAMPLE_CHARS: usize = 40;
const MAX_SAMPLES: usize = 3;

const LEAD_PREFIXES: [&str; 8] = ["令人", "更", "真正", "值得", "不难", "可以", "或许", "也许"];
const LEAD_ADJECTIVES: [&str; 11] = [
    "不安", "可怕", "遗憾", "欣慰", "意外", "讽刺", "悲哀", "庆幸", "重要", "关键", "明显",
];
const LEAD_DIRECT: [&str; 3] = ["事实上", "换句话说", "要知道"];

const BODY_CLICHES: [&str; 10] = [
    "心猛地一沉",
    "心头一沉",
    "一股暖流",
    "指尖发凉",
    "血液凝固",
    "如遭雷击",
    "呼吸一滞",
    "脊背发凉",
    "浑身一震",
    "心脏漏跳",
];

const GUIDANCE_WORDS: [&str; 8] = [
    "终于明白",
    "这一刻",
    "原来",
    "或许这就是",
    "也许这就是",
    "从此以后",
    "他知道",
    "她懂了",
];
const THEME_WORDS: [&str; 8] = ["人生", "命运", "爱", "成长", "时间", "世界", "岁月", "救赎"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintKind {
    LeadComment,
    NotBut,
    Enumeration,
    BodyCliche,
    DialoguePingPong,
    RepeatedPhrase,
    EndingMoral,
}

impl LintKind {
    /// 稳定英文标识，用于任务结果持久化。
    pub fn as_str(self) -> &'static str {
        match self {
            LintKind::LeadComment => "lead_comment",
            LintKind::NotBut => "not_but",
            LintKind::Enumeration => "enumeration",
            LintKind::BodyCliche => "body_cliche",
            LintKind::DialoguePingPong => "dialogue_ping_pong",
            LintKind::RepeatedPhrase => "repeated_phrase",
            LintKind::EndingMoral => "ending_moral",
        }
    }

    /// 中文短名，用于任务结果展示。
    pub fn label(self) -> &'static str {
        match self {
            LintKind::LeadComment => "评论腔",
            LintKind::NotBut => "翻案腔",
            LintKind::Enumeration => "抽象罗列",
            LintKind::BodyCliche => "身体套话",
            LintKind::DialoguePingPong => "对白乒乓",
            LintKind::RepeatedPhrase => "重复句式",
            LintKind::EndingMoral => "结尾升华",
        }
    }

    /// 更完整的说明，直接用于重写 prompt 的 reasons 变量。
    pub fn reason(self) -> &'static str {
        match self {
            LintKind::LeadComment => "评论腔：用“令人……的是/事实上”等旁白代替事实",
            LintKind::NotBut => "翻案腔：使用“不是……而是……/与其说……不如说……”式判断",
            LintKind::Enumeration => "抽象罗列：用顿号堆叠三个以上抽象名词",
            LintKind::BodyCliche => "身体套话：心猛地一沉、一股暖流、指尖发凉等通用反应",
            LintKind::DialoguePingPong => "对白乒乓：连续多段纯对白，缺少动作或环境承托",
            LintKind::RepeatedPhrase => "重复句式：同一三字片段反复出现",
            LintKind::EndingMoral => "结尾升华：在结尾总结主题或提炼意义",
        }
    }

    fn is_hard(self) -> bool {
        matches!(
            self,
            LintKind::LeadComment
                | LintKind::NotBut
                | LintKind::Enumeration
                | LintKind::BodyCliche
                | LintKind::EndingMoral
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintIssue {
    pub kind: LintKind,
    pub count: i64,
    pub samples: Vec<String>,
    pub paragraphs: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LintReport {
    pub total: i64,
    pub issues: Vec<LintIssue>,
}

/// 正文段落：按行切分，trim 后非空即为一段，索引从 0 开始。
pub fn lint(text: &str) -> LintReport {
    let paragraphs = split_paragraphs(text);
    if paragraphs.is_empty() {
        return LintReport::default();
    }

    let mut issues = Vec::new();
    for issue in [
        detect_lead_comment(&paragraphs),
        detect_not_but(&paragraphs),
        detect_enumeration(&paragraphs),
        detect_body_cliche(&paragraphs),
        detect_dialogue_ping_pong(&paragraphs),
        detect_repeated_phrase(text, &paragraphs),
        detect_ending_moral(&paragraphs),
    ]
    .into_iter()
    .flatten()
    {
        issues.push(issue);
    }

    let total = issues.iter().map(|issue| issue.count).sum();
    LintReport { total, issues }
}

/// 任一硬规则命中，或软规则累计达到 4 次。
pub fn needs_rewrite(report: &LintReport) -> bool {
    report.issues.iter().any(|issue| issue.kind.is_hard()) || report.total >= 4
}

/// 选出需要定点重写的段落：硬规则优先，同一段合并命中类型，最多 `max` 段。
pub fn rewrite_targets(
    text: &str,
    report: &LintReport,
    max: usize,
) -> Vec<(usize, String, Vec<LintKind>)> {
    if max == 0 {
        return Vec::new();
    }

    let paragraphs = split_paragraphs(text);
    let mut ordered: Vec<&LintIssue> = report.issues.iter().collect();
    ordered.sort_by_key(|issue| !issue.kind.is_hard());

    let mut selected: Vec<(usize, Vec<LintKind>)> = Vec::new();
    for issue in ordered {
        for &index in &issue.paragraphs {
            if index >= paragraphs.len() {
                continue;
            }
            if let Some((_, kinds)) = selected.iter_mut().find(|(selected, _)| *selected == index) {
                if !kinds.contains(&issue.kind) {
                    kinds.push(issue.kind);
                }
                continue;
            }
            if selected.len() < max {
                selected.push((index, vec![issue.kind]));
            }
        }
    }

    selected
        .into_iter()
        .map(|(index, kinds)| (index, paragraphs[index].to_string(), kinds))
        .collect()
}

fn split_paragraphs(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn split_sentences(paragraph: &str) -> Vec<&str> {
    paragraph
        .split(['。', '！', '？', '!', '?'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .collect()
}

fn sample(text: &str) -> String {
    text.chars().take(SAMPLE_CHARS).collect()
}

fn push_sample(samples: &mut Vec<String>, text: &str) {
    if samples.len() < MAX_SAMPLES {
        samples.push(sample(text));
    }
}

fn detect_lead_comment(paragraphs: &[&str]) -> Option<LintIssue> {
    let mut count = 0i64;
    let mut samples = Vec::new();
    let mut indices = Vec::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        if !has_lead_comment(paragraph) {
            continue;
        }
        count += 1;
        push_sample(&mut samples, paragraph);
        indices.push(index);
    }
    issue_if_hit(LintKind::LeadComment, count, samples, indices)
}

fn has_lead_comment(paragraph: &str) -> bool {
    let paragraph = paragraph.trim_start();
    for direct in LEAD_DIRECT {
        if let Some(rest) = paragraph.strip_prefix(direct) {
            if starts_with_comma(rest) {
                return true;
            }
        }
    }
    for prefix in std::iter::once("").chain(LEAD_PREFIXES) {
        let Some(rest) = paragraph.strip_prefix(prefix) else {
            continue;
        };
        for adjective in LEAD_ADJECTIVES {
            let Some(rest) = rest.strip_prefix(adjective) else {
                continue;
            };
            let Some(rest) = rest.strip_prefix("的是") else {
                continue;
            };
            if starts_with_comma(rest) {
                return true;
            }
        }
    }
    false
}

fn starts_with_comma(text: &str) -> bool {
    text.starts_with('，') || text.starts_with(',')
}

fn detect_not_but(paragraphs: &[&str]) -> Option<LintIssue> {
    let mut count = 0i64;
    let mut samples = Vec::new();
    let mut indices = Vec::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let mut hit = false;
        for sentence in split_sentences(paragraph) {
            if !has_not_but(sentence) {
                continue;
            }
            hit = true;
            count += 1;
            push_sample(&mut samples, sentence);
        }
        if hit {
            indices.push(index);
        }
    }
    issue_if_hit(LintKind::NotBut, count, samples, indices)
}

fn has_not_but(sentence: &str) -> bool {
    const PAIRS: [(&str, &str); 3] = [("不是", "而是"), ("并非", "而是"), ("与其说", "不如说")];
    PAIRS.iter().any(
        |(left, right)| match (sentence.find(left), sentence.find(right)) {
            (Some(left_at), Some(right_at)) => left_at < right_at,
            _ => false,
        },
    )
}

fn detect_enumeration(paragraphs: &[&str]) -> Option<LintIssue> {
    let mut count = 0i64;
    let mut samples = Vec::new();
    let mut indices = Vec::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let mut hit = false;
        for sentence in split_sentences(paragraph) {
            if !has_abstract_enumeration(sentence) {
                continue;
            }
            hit = true;
            count += 1;
            push_sample(&mut samples, sentence);
        }
        if hit {
            indices.push(index);
        }
    }
    issue_if_hit(LintKind::Enumeration, count, samples, indices)
}

fn has_abstract_enumeration(sentence: &str) -> bool {
    if sentence.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    if sentence.matches('、').count() < 2 {
        return false;
    }
    let core = sentence.trim_end_matches(is_punct);
    let items: Vec<&str> = core.split('、').map(str::trim).collect();
    if items.len() < 3 {
        return false;
    }
    items.iter().all(|item| {
        let length = item.chars().count();
        (2..=4).contains(&length) && item.chars().all(is_han)
    })
}

fn detect_body_cliche(paragraphs: &[&str]) -> Option<LintIssue> {
    let mut count = 0i64;
    let mut samples = Vec::new();
    let mut indices = Vec::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let hits: usize = BODY_CLICHES
            .iter()
            .map(|phrase| paragraph.match_indices(phrase).count())
            .sum();
        if hits == 0 {
            continue;
        }
        count += hits as i64;
        push_sample(&mut samples, paragraph);
        indices.push(index);
    }
    issue_if_hit(LintKind::BodyCliche, count, samples, indices)
}

fn detect_dialogue_ping_pong(paragraphs: &[&str]) -> Option<LintIssue> {
    let mut count = 0i64;
    let mut samples = Vec::new();
    let mut indices = Vec::new();

    let mut run_start = 0usize;
    let mut run_length = 0usize;
    for index in 0..=paragraphs.len() {
        let is_dialogue = index < paragraphs.len() && is_pure_dialogue(paragraphs[index]);
        if is_dialogue {
            if run_length == 0 {
                run_start = index;
            }
            run_length += 1;
            continue;
        }
        if run_length >= 4 {
            count += run_length as i64;
            push_sample(&mut samples, paragraphs[run_start]);
            indices.extend(run_start..run_start + run_length);
        }
        run_length = 0;
    }

    issue_if_hit(LintKind::DialoguePingPong, count, samples, indices)
}

fn is_pure_dialogue(line: &str) -> bool {
    let line = line.trim();
    let Some(first) = line.chars().next() else {
        return false;
    };
    if !matches!(first, '「' | '“' | '"') {
        return false;
    }
    strip_dialogue(line).chars().all(is_punct)
}

/// 去掉成对引号内的对白，只留下引号外的叙述文字。
fn strip_dialogue(line: &str) -> String {
    let mut outside = String::new();
    let mut chars = line.chars();
    while let Some(character) = chars.next() {
        let closing = match character {
            '「' => Some('」'),
            '“' => Some('”'),
            '"' => Some('"'),
            _ => None,
        };
        let Some(closing) = closing else {
            outside.push(character);
            continue;
        };
        for inner in chars.by_ref() {
            if inner == closing {
                break;
            }
        }
    }
    outside
}

fn detect_repeated_phrase(text: &str, paragraphs: &[&str]) -> Option<LintIssue> {
    let char_count = text.chars().filter(|c| !c.is_whitespace()).count() as i64;
    let threshold = (char_count / 400).max(5);

    let mut counts: HashMap<String, (i64, Vec<usize>)> = HashMap::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let chars: Vec<char> = paragraph.chars().collect();
        if chars.len() < 3 {
            continue;
        }
        for window in chars.windows(3) {
            let fragment: String = window.iter().collect();
            if fragment.chars().all(is_punct) {
                continue;
            }
            let (count, indices) = counts.entry(fragment).or_insert((0, Vec::new()));
            *count += 1;
            if indices.last().copied() != Some(index) {
                indices.push(index);
            }
        }
    }

    let mut offenders: Vec<(String, i64, Vec<usize>)> = counts
        .into_iter()
        .filter(|(_, (count, _))| *count >= threshold)
        .map(|(fragment, (count, indices))| (fragment, count, indices))
        .collect();
    if offenders.is_empty() {
        return None;
    }
    offenders.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    let samples = offenders
        .iter()
        .take(MAX_SAMPLES)
        .map(|(fragment, _, _)| sample(fragment))
        .collect();
    let mut indices: Vec<usize> = offenders
        .iter()
        .flat_map(|(_, _, indices)| indices.iter().copied())
        .collect();
    indices.sort_unstable();
    indices.dedup();

    Some(LintIssue {
        kind: LintKind::RepeatedPhrase,
        count: offenders.len() as i64,
        samples,
        paragraphs: indices,
    })
}

fn detect_ending_moral(paragraphs: &[&str]) -> Option<LintIssue> {
    let start = paragraphs.len().saturating_sub(2);
    let tail = &paragraphs[start..];
    let region = tail.join("\n");
    let has_guidance = GUIDANCE_WORDS.iter().any(|word| region.contains(word));
    let has_theme = THEME_WORDS.iter().any(|word| region.contains(word));
    if !has_guidance || !has_theme {
        return None;
    }

    let mut samples = Vec::new();
    let mut indices = Vec::new();
    for (offset, paragraph) in tail.iter().enumerate() {
        let has_guidance = GUIDANCE_WORDS.iter().any(|word| paragraph.contains(word));
        let has_theme = THEME_WORDS.iter().any(|word| paragraph.contains(word));
        if !has_guidance && !has_theme {
            continue;
        }
        indices.push(start + offset);
        push_sample(&mut samples, paragraph);
    }

    Some(LintIssue {
        kind: LintKind::EndingMoral,
        count: indices.len() as i64,
        samples,
        paragraphs: indices,
    })
}

fn issue_if_hit(
    kind: LintKind,
    count: i64,
    samples: Vec<String>,
    paragraphs: Vec<usize>,
) -> Option<LintIssue> {
    if count == 0 {
        return None;
    }
    Some(LintIssue {
        kind,
        count,
        samples,
        paragraphs,
    })
}

fn is_han(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'
    )
}

fn is_punct(character: char) -> bool {
    character.is_whitespace()
        || character.is_ascii_punctuation()
        || matches!(
            character,
            '，' | '。'
                | '！'
                | '？'
                | '、'
                | '；'
                | '：'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '《'
                | '》'
                | '…'
                | '—'
                | '·'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue_kinds(report: &LintReport) -> Vec<LintKind> {
        report.issues.iter().map(|issue| issue.kind).collect()
    }

    #[test]
    fn lead_comment_needs_paragraph_start_and_comma() {
        let report = lint("更可怕的是，雨停了。\n他推门而入。");
        assert_eq!(issue_kinds(&report), vec![LintKind::LeadComment]);
        assert_eq!(report.issues[0].count, 1);
        assert_eq!(report.issues[0].paragraphs, vec![0]);

        assert!(lint("他推门而入，雨已经停了。").issues.is_empty());
        assert!(lint("事实上他推门走了。").issues.is_empty());
    }

    #[test]
    fn not_but_must_share_one_sentence() {
        let report = lint("这不是害怕，而是兴奋。\n他不是在等人，而是在等答案。");
        assert_eq!(issue_kinds(&report), vec![LintKind::NotBut]);
        assert_eq!(report.issues[0].count, 2);
        assert_eq!(report.issues[0].paragraphs, vec![0, 1]);

        assert!(lint("不是害怕。而是兴奋。").issues.is_empty());
        assert!(lint("他既害怕又兴奋。").issues.is_empty());
    }

    #[test]
    fn enumeration_only_matches_short_abstract_items() {
        let report = lint("孤独、迷茫、愤怒。");
        assert_eq!(issue_kinds(&report), vec![LintKind::Enumeration]);
        assert_eq!(report.issues[0].count, 1);

        assert!(lint("买了盐、米、油。").issues.is_empty());
        assert!(lint("孤独、迷茫、愤怒在胸口翻涌。").issues.is_empty());
    }

    #[test]
    fn body_cliche_matches_only_listed_phrases() {
        let report = lint("他心猛地一沉。\n指尖发凉。");
        assert_eq!(issue_kinds(&report), vec![LintKind::BodyCliche]);
        assert_eq!(report.issues[0].count, 2);

        assert!(lint("他数错了第三级台阶。").issues.is_empty());
    }

    #[test]
    fn dialogue_ping_pong_needs_four_consecutive_pure_lines() {
        let report = lint("「你来了。」\n「我来了。」\n「东西呢。」\n「在这里。」");
        assert_eq!(issue_kinds(&report), vec![LintKind::DialoguePingPong]);
        assert_eq!(report.issues[0].count, 4);
        assert_eq!(report.issues[0].paragraphs, vec![0, 1, 2, 3]);

        assert!(lint("「你来了。」\n「我来了。」\n「东西呢。」")
            .issues
            .is_empty());
        assert!(lint("「你来了。」\n「我来了。」\n「东西呢。」\n他点头。")
            .issues
            .is_empty());
    }

    #[test]
    fn repeated_phrase_respects_threshold() {
        let repeated = "他抬起头。\n他抬起头。\n他抬起头。\n他抬起头。\n他抬起头。";
        let report = lint(repeated);
        let issue = report
            .issues
            .iter()
            .find(|issue| issue.kind == LintKind::RepeatedPhrase)
            .expect("repeated phrase");
        assert!(issue.count >= 1);
        assert!(issue.samples.len() <= 3);
        assert!(issue
            .samples
            .iter()
            .all(|sample| sample.chars().count() <= 40));

        let below = "他抬起头。\n他抬起头。\n他抬起头。\n他抬起头。";
        assert!(!issue_kinds(&lint(below)).contains(&LintKind::RepeatedPhrase));
    }

    #[test]
    fn ending_moral_requires_both_word_groups() {
        let report = lint("他站在街口。\n他终于明白，这就是人生。");
        assert_eq!(issue_kinds(&report), vec![LintKind::EndingMoral]);

        assert!(lint("他站在街口。\n他终于明白，雨停了。").issues.is_empty());
        assert!(lint("他站在街口。\n这就是人生。").issues.is_empty());
    }

    #[test]
    fn needs_rewrite_on_hard_rule_or_four_soft_hits() {
        let hard = lint("这不是害怕，而是兴奋。");
        assert!(needs_rewrite(&hard));

        let soft_four = LintReport {
            total: 4,
            issues: vec![LintIssue {
                kind: LintKind::DialoguePingPong,
                count: 4,
                samples: Vec::new(),
                paragraphs: Vec::new(),
            }],
        };
        assert!(needs_rewrite(&soft_four));

        let soft_three = LintReport {
            total: 3,
            issues: vec![LintIssue {
                kind: LintKind::DialoguePingPong,
                count: 3,
                samples: Vec::new(),
                paragraphs: Vec::new(),
            }],
        };
        assert!(!needs_rewrite(&soft_three));
    }

    #[test]
    fn rewrite_targets_merge_kinds_and_respect_limit() {
        let text = "令人不安的是，这不是害怕，而是兴奋。\n第二段。\n第三段。";
        let report = lint(text);
        let targets = rewrite_targets(text, &report, 3);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].0, 0);
        assert!(targets[0].2.contains(&LintKind::LeadComment));
        assert!(targets[0].2.contains(&LintKind::NotBut));

        assert!(rewrite_targets(text, &report, 0).is_empty());
    }
}
