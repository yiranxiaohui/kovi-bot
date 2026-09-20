use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::OnceLock;

const TERMS_JSON: &str = include_str!("../assets/official_terms.zhs.json");

#[derive(Debug, Deserialize)]
struct TermsDocument {
    terms: Vec<RawTerm>,
}

#[derive(Debug, Deserialize)]
struct RawTerm {
    english: String,
    variants: Vec<TermVariant>,
}

#[derive(Debug, Deserialize)]
struct TermVariant {
    chinese: String,
    #[serde(default)]
    sources: Vec<String>,
}

#[derive(Debug)]
struct OfficialTerm {
    english: String,
    variants: Vec<TermVariant>,
    pattern: Regex,
}

pub struct MatchedTerms<'a> {
    terms: Vec<&'a OfficialTerm>,
}

fn official_terms() -> &'static [OfficialTerm] {
    static TERMS: OnceLock<Vec<OfficialTerm>> = OnceLock::new();
    TERMS.get_or_init(|| {
        let document: TermsDocument =
            serde_json::from_str(TERMS_JSON).expect("内置的杀戮尖塔 2 术语表必须是有效 JSON");
        document
            .terms
            .into_iter()
            .map(|term| {
                let flexible_name = term
                    .english
                    .split_whitespace()
                    .map(regex::escape)
                    .collect::<Vec<_>>()
                    .join(r"[\s_-]*");
                let pattern = Regex::new(&format!(r"(?i)\b{flexible_name}\b"))
                    .expect("转义后的官方术语必须能编译为正则表达式");
                OfficialTerm {
                    english: term.english,
                    variants: term.variants,
                    pattern,
                }
            })
            .collect()
    })
}

impl MatchedTerms<'_> {
    pub fn for_source(source: &str) -> Self {
        Self {
            terms: official_terms()
                .iter()
                .filter(|term| term.pattern.is_match(source))
                .collect(),
        }
    }

    pub fn prompt_glossary(&self) -> String {
        if self.terms.is_empty() {
            return "（本公告没有匹配到实体专名）".to_string();
        }

        self.terms
            .iter()
            .map(|term| {
                let variants = term
                    .variants
                    .iter()
                    .map(|variant| {
                        if term.variants.len() == 1 {
                            variant.chinese.clone()
                        } else {
                            format!("{}（{}）", variant.chinese, source_labels(&variant.sources))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" / ");
                format!("- {} = {variants}", term.english)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn apply_unambiguous(&self, text: &str) -> String {
        let mut translated = text.to_string();
        let mut chinese_names = BTreeSet::new();
        for term in &self.terms {
            let [variant] = term.variants.as_slice() else {
                continue;
            };
            translated = term
                .pattern
                .replace_all(&translated, variant.chinese.as_str())
                .into_owned();
            chinese_names.insert(variant.chinese.as_str());
        }

        for chinese in chinese_names {
            for duplicate in [
                format!("{chinese}（{chinese}）"),
                format!("{chinese}({chinese})"),
                format!("{chinese} ({chinese})"),
            ] {
                translated = translated.replace(&duplicate, chinese);
            }
        }
        translated
    }
}

fn source_labels(sources: &[String]) -> String {
    sources
        .iter()
        .map(|source| match source.as_str() {
            "cards" => "卡牌",
            "characters" | "translations/character_names" => "角色",
            "relics" => "遗物",
            "monsters" => "怪物",
            "monster_moves" => "怪物招式",
            "potions" => "药水",
            "enchantments" => "附魔",
            "encounters" => "遭遇",
            "events" => "事件",
            "powers" => "能力",
            "keywords" | "translations/keywords" => "关键词",
            "intents" => "意图",
            "orbs" => "充能球",
            "afflictions" => "负面效果",
            "modifiers" => "修正项",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("、")
}

#[cfg(test)]
mod tests {
    use super::MatchedTerms;

    #[test]
    fn official_names_are_injected_and_enforced() {
        let terms = MatchedTerms::for_source(
            "Buffed Axebot: Hammer Uppercut and The One-Two. Adjusted Rend and Splash.",
        );
        let glossary = terms.prompt_glossary();
        assert!(glossary.contains("Axebot = 巨斧机器人"));
        assert!(glossary.contains("Hammer Uppercut = 上勾锤击"));
        assert!(glossary.contains("Rend = 撕碎"));

        assert_eq!(
            terms.apply_unambiguous("Axebot 使用 Hammer Uppercut；Rend 与 Splash。"),
            "巨斧机器人 使用 上勾锤击；撕碎 与 飞溅。"
        );
    }

    #[test]
    fn names_with_spacing_differences_still_match() {
        let terms = MatchedTerms::for_source("Buffed Mechaknight.");
        assert!(terms.prompt_glossary().contains("Mecha Knight = 机甲骑士"));
        assert_eq!(terms.apply_unambiguous("Mechaknight"), "机甲骑士");
    }

    #[test]
    fn ambiguous_names_are_left_to_the_translator_with_context() {
        let terms = MatchedTerms::for_source("Bash");
        let glossary = terms.prompt_glossary();
        assert!(glossary.contains("Bash ="));
        assert!(glossary.contains("痛击（卡牌）"));
        assert!(glossary.contains("猛击（怪物招式）"));
        assert_eq!(terms.apply_unambiguous("Bash"), "Bash");
    }
}
