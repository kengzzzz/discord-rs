#[derive(Debug, Clone)]
pub(super) struct ScoreResult {
    pub is_spam: bool,
    pub risk: f32,
    pub action: String,
    pub score_raw: i32,
    pub reasons: Vec<String>,
}

pub(super) fn score_text(text: &str, block_threshold: f32, review_threshold: f32) -> ScoreResult {
    let normalized = normalize(text);
    let mut score = 0;
    let mut reasons = Vec::new();

    let terms = [
        ("crypto", 3),
        ("bitcoin", 3),
        ("btc", 3),
        ("eth", 3),
        ("usdt", 4),
        ("wallet", 3),
        ("deposit", 3),
        ("withdraw", 3),
        ("withdrawal", 3),
        ("withdrawal success", 6),
        ("transaction", 2),
        ("casino", 4),
        ("gambling", 4),
        ("rakeback", 5),
        ("bonus", 3),
        ("free bonus", 5),
        ("promo code", 5),
        ("activate code", 5),
        ("activation code", 5),
        ("claim reward", 5),
        ("claim now", 5),
        ("register now", 4),
        ("limited time", 4),
        ("guaranteed", 3),
        ("guaranteed profit", 6),
        ("free money", 6),
        ("double your", 5),
        ("investment", 3),
        ("profit", 3),
        ("airdrop", 4),
    ];

    for (term, weight) in terms {
        if normalized.contains(term) {
            score += weight;
            reasons.push(term.to_owned());
        }
    }

    add_pattern(
        &normalized,
        &mut score,
        &mut reasons,
        "money/bonus-withdraw-deposit pattern",
        6,
        has_any(&normalized, &["$", "usd", "usdt"])
            && has_any(
                &normalized,
                &["bonus", "withdraw", "withdrawal", "deposit"],
            ),
    );
    add_pattern(
        &normalized,
        &mut score,
        &mut reasons,
        "activation/promo code pattern",
        6,
        normalized.contains("code")
            && has_any(
                &normalized,
                &["activate", "activation", "bonus", "promo", "claim"],
            ),
    );
    add_pattern(
        &normalized,
        &mut score,
        &mut reasons,
        "crypto deposit-withdraw pattern",
        7,
        has_any(
            &normalized,
            &["crypto", "usdt", "btc", "eth", "wallet"],
        ) && has_any(
            &normalized,
            &["deposit", "withdraw", "withdrawal"],
        ),
    );
    add_pattern(
        &normalized,
        &mut score,
        &mut reasons,
        "social reward-promo pattern",
        5,
        has_any(
            &normalized,
            &["discord", "telegram", "twitter", "x.com"],
        ) && has_any(
            &normalized,
            &["reward", "bonus", "promo", "claim", "airdrop"],
        ),
    );
    add_pattern(
        &normalized,
        &mut score,
        &mut reasons,
        "casino bonus-deposit pattern",
        6,
        has_any(
            &normalized,
            &["casino", "gambling", "bet", "wager"],
        ) && has_any(
            &normalized,
            &["bonus", "rakeback", "deposit", "withdraw"],
        ),
    );
    add_pattern(
        &normalized,
        &mut score,
        &mut reasons,
        "high-value urgency pattern",
        6,
        has_any(
            &normalized,
            &["1000", "10000", "million", "guaranteed", "double your"],
        ) && has_any(
            &normalized,
            &["now", "limited time", "today", "hurry", "expires"],
        ),
    );

    reasons.sort();
    reasons.dedup();

    let risk = (score as f32 / 20.0).clamp(0.0, 1.0);
    let action = if risk >= block_threshold {
        "block"
    } else if risk >= review_threshold {
        "review"
    } else {
        "allow"
    };

    ScoreResult {
        is_spam: action == "block",
        risk,
        action: action.to_owned(),
        score_raw: score,
        reasons,
    }
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(
            |ch| {
                if ch.is_ascii_alphanumeric() || ch == '$' || ch == '.' { ch } else { ' ' }
            },
        )
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_any(text: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| text.contains(needle))
}

fn add_pattern(
    text: &str,
    score: &mut i32,
    reasons: &mut Vec<String>,
    reason: &str,
    weight: i32,
    matched: bool,
) {
    if matched && !text.is_empty() {
        *score += weight;
        reasons.push(reason.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK_THRESHOLD: f32 = 0.80;
    const REVIEW_THRESHOLD: f32 = 0.55;

    fn score(text: &str) -> ScoreResult {
        score_text(text, BLOCK_THRESHOLD, REVIEW_THRESHOLD)
    }

    #[test]
    fn scam_like_text_blocks() {
        let result = score(
            "Withdrawal success! Claim reward now. USDT wallet deposit bonus promo code limited \
             time guaranteed profit.",
        );

        assert_eq!(result.action, "block");
        assert!(result.is_spam);
        assert!(result.risk >= BLOCK_THRESHOLD);
        assert!(
            result
                .reasons
                .iter()
                .any(|reason| reason == "usdt")
        );
    }

    #[test]
    fn normal_text_allows() {
        let result =
            score("Here is the meeting agenda for tomorrow. Please review the design notes.");

        assert_eq!(result.action, "allow");
        assert!(!result.is_spam);
        assert!(result.risk < REVIEW_THRESHOLD);
    }

    #[test]
    fn review_like_text_reviews() {
        let result = score("Register now for a limited time bonus offer.");

        assert_eq!(result.action, "review");
        assert!(!result.is_spam);
        assert!(result.risk >= REVIEW_THRESHOLD && result.risk < BLOCK_THRESHOLD);
    }

    #[test]
    fn matching_is_case_insensitive_and_reasons_are_unique() {
        let upper = score("FREE BONUS PROMO CODE CLAIM NOW PROMO CODE");
        let lower = score("free bonus promo code claim now promo code");

        assert_eq!(upper.score_raw, lower.score_raw);
        assert_eq!(upper.action, lower.action);
        assert_eq!(
            upper
                .reasons
                .iter()
                .filter(|reason| reason.as_str() == "promo code")
                .count(),
            1
        );
    }

    #[test]
    fn thresholds_are_inclusive() {
        let review = score_text("register now limited time bonus", 1.0, 0.55);
        let block = score_text("register now limited time bonus", 0.55, 0.1);

        assert_eq!(review.action, "review");
        assert_eq!(block.action, "block");
    }
}
