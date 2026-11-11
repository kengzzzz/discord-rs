#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Allow,
    Review,
    Block,
}

#[derive(Clone, Debug)]
pub struct Score {
    pub risk: f32,
    pub action: Action,
    pub raw: i32,
    pub reasons: Vec<&'static str>,
}

const TERMS: [(&str, i32); 30] = [
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

const WHOLE_WORDS: [&str; 5] = ["eth", "btc", "bet", "usd", "now"];

fn contains(text: &str, needle: &str) -> bool {
    if !WHOLE_WORDS.contains(&needle) {
        return text.contains(needle);
    }
    text.match_indices(needle)
        .any(|(start, _)| {
            let before = text[..start].chars().next_back();
            let after = text[start + needle.len()..]
                .chars()
                .next();
            !before.is_some_and(|c| c.is_ascii_alphanumeric())
                && !after.is_some_and(|c| c.is_ascii_alphanumeric())
        })
}

pub fn score_text(text: &str, block_threshold: f32, review_threshold: f32) -> Score {
    let normalized = normalize(text);
    let mut raw = 0;
    let mut reasons = Vec::new();

    for (term, weight) in TERMS {
        if contains(&normalized, term) {
            raw += weight;
            reasons.push(term);
        }
    }

    let patterns: [(&'static str, i32, bool); 6] = [
        (
            "money/bonus-withdraw-deposit pattern",
            6,
            has_any(&normalized, &["$", "usd", "usdt"])
                && has_any(
                    &normalized,
                    &["bonus", "withdraw", "withdrawal", "deposit"],
                ),
        ),
        (
            "activation/promo code pattern",
            6,
            normalized.contains("code")
                && has_any(
                    &normalized,
                    &["activate", "activation", "bonus", "promo", "claim"],
                ),
        ),
        (
            "crypto deposit-withdraw pattern",
            7,
            has_any(
                &normalized,
                &["crypto", "usdt", "btc", "eth", "wallet"],
            ) && has_any(
                &normalized,
                &["deposit", "withdraw", "withdrawal"],
            ),
        ),
        (
            "social reward-promo pattern",
            5,
            has_any(
                &normalized,
                &["discord", "telegram", "twitter", "x.com"],
            ) && has_any(
                &normalized,
                &["reward", "bonus", "promo", "claim", "airdrop"],
            ),
        ),
        (
            "casino bonus-deposit pattern",
            6,
            has_any(
                &normalized,
                &["casino", "gambling", "bet", "wager"],
            ) && has_any(
                &normalized,
                &["bonus", "rakeback", "deposit", "withdraw"],
            ),
        ),
        (
            "high-value urgency pattern",
            6,
            has_any(
                &normalized,
                &["1000", "10000", "million", "guaranteed", "double your"],
            ) && has_any(
                &normalized,
                &["now", "limited time", "today", "hurry", "expires"],
            ),
        ),
    ];
    for (reason, weight, matched) in patterns {
        if matched && !normalized.is_empty() {
            raw += weight;
            reasons.push(reason);
        }
    }

    reasons.sort_unstable();
    reasons.dedup();

    let risk = (raw as f32 / 20.0).clamp(0.0, 1.0);
    let action = if risk >= block_threshold {
        Action::Block
    } else if risk >= review_threshold {
        Action::Review
    } else {
        Action::Allow
    };
    Score { risk, action, raw, reasons }
}

fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text
        .chars()
        .flat_map(char::to_lowercase)
    {
        if ch.is_ascii_alphanumeric() || ch == '$' || ch == '.' {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(ch);
        } else {
            pending_space = true;
        }
    }
    out
}

fn has_any(text: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| contains(text, needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: f32 = 0.80;
    const REVIEW: f32 = 0.55;

    fn score(text: &str) -> Score {
        score_text(text, BLOCK, REVIEW)
    }

    #[test]
    fn scam_like_text_blocks() {
        let result = score(
            "Withdrawal success! Claim reward now. USDT wallet deposit bonus promo code limited \
             time guaranteed profit.",
        );
        assert_eq!(result.action, Action::Block);
        assert!(result.risk >= BLOCK);
        assert!(result.reasons.contains(&"usdt"));
    }

    #[test]
    fn normal_text_allows() {
        let result =
            score("Here is the meeting agenda for tomorrow. Please review the design notes.");
        assert_eq!(result.action, Action::Allow);
        assert!(result.risk < REVIEW);
    }

    #[test]
    fn review_like_text_reviews() {
        let result = score("Register now for a limited time bonus offer.");
        assert_eq!(result.action, Action::Review);
    }

    #[test]
    fn matching_is_case_insensitive_and_reasons_are_unique() {
        let upper = score("FREE BONUS PROMO CODE CLAIM NOW PROMO CODE");
        let lower = score("free bonus promo code claim now promo code");
        assert_eq!(upper.raw, lower.raw);
        assert_eq!(upper.action, lower.action);
        assert_eq!(
            upper
                .reasons
                .iter()
                .filter(|r| **r == "promo code")
                .count(),
            1
        );
    }

    #[test]
    fn short_terms_only_match_whole_words() {
        let checkout = score(
            "Choose a payment method between card and bank. Deposit or withdraw anytime, you know.",
        );
        assert_eq!(
            checkout.action,
            Action::Allow,
            "{:?}",
            checkout.reasons
        );
        assert!(!checkout.reasons.contains(&"eth"));
        let crypto = score("Send ETH or BTC to your wallet, deposit now");
        assert!(crypto.reasons.contains(&"eth") && crypto.reasons.contains(&"btc"));
    }

    #[test]
    fn thresholds_are_inclusive() {
        assert_eq!(
            score_text("register now limited time bonus", 1.0, 0.55).action,
            Action::Review
        );
        assert_eq!(
            score_text("register now limited time bonus", 0.55, 0.1).action,
            Action::Block
        );
    }

    #[test]
    fn normalization_collapses_punctuation_like_the_original() {
        assert_eq!(
            normalize("  Promo-CODE!!  $100\n\tUSDT "),
            "promo code $100 usdt"
        );
    }
}
