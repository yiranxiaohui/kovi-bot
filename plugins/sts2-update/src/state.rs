use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchMode {
    All,
    Stable,
    Beta,
}

impl BranchMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部（正式版 + 测试版）",
            Self::Stable => "仅正式版",
            Self::Beta => "仅测试版",
        }
    }

    pub fn accepts(self, is_beta: bool) -> bool {
        match self {
            Self::All => true,
            Self::Stable => !is_beta,
            Self::Beta => is_beta,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Subscription {
    pub group_id: i64,
    pub mode: BranchMode,
    #[serde(default)]
    pub initialized: bool,
    #[serde(default)]
    pub seen_gids: BTreeSet<String>,
}

impl Subscription {
    pub fn new(group_id: i64, mode: BranchMode) -> Self {
        Self {
            group_id,
            mode,
            initialized: false,
            seen_gids: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct PersistentState {
    pub subscriptions: Vec<Subscription>,
    pub translations: BTreeMap<String, String>,
}

impl PersistentState {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            let state = Self::default();
            state.save(path)?;
            return Ok(state);
        }
        let contents =
            std::fs::read_to_string(path).map_err(|e| format!("读取订阅状态失败: {e}"))?;
        serde_json::from_str(&contents).map_err(|e| format!("解析订阅状态失败: {e}"))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| "订阅状态路径没有父目录".to_string())?;
        std::fs::create_dir_all(parent).map_err(|e| format!("创建插件数据目录失败: {e}"))?;
        let temporary = path.with_extension("json.tmp");
        let contents = serde_json::to_vec(self).map_err(|e| format!("序列化订阅状态失败: {e}"))?;
        std::fs::write(&temporary, contents).map_err(|e| format!("写入订阅状态失败: {e}"))?;
        std::fs::rename(&temporary, path).map_err(|e| format!("提交订阅状态失败: {e}"))
    }

    pub fn subscription(&self, group_id: i64) -> Option<&Subscription> {
        self.subscriptions.iter().find(|s| s.group_id == group_id)
    }

    pub fn subscription_mut(&mut self, group_id: i64) -> Option<&mut Subscription> {
        self.subscriptions
            .iter_mut()
            .find(|s| s.group_id == group_id)
    }

    pub fn retain_translation_namespace(&mut self, namespace: &str) -> bool {
        let prefix = format!("{namespace}:");
        let old_len = self.translations.len();
        self.translations.retain(|key, _| key.starts_with(&prefix));
        self.translations.len() != old_len
    }
}

#[cfg(test)]
mod tests {
    use super::{BranchMode, PersistentState, Subscription};

    #[test]
    fn branch_mode_filters_expected_branch() {
        assert!(BranchMode::All.accepts(true));
        assert!(BranchMode::All.accepts(false));
        assert!(BranchMode::Stable.accepts(false));
        assert!(!BranchMode::Stable.accepts(true));
        assert!(BranchMode::Beta.accepts(true));
        assert!(!BranchMode::Beta.accepts(false));
    }

    #[test]
    fn state_round_trips_integer_group_ids() {
        let mut state = PersistentState::default();
        state
            .subscriptions
            .push(Subscription::new(123_456, BranchMode::All));
        let json = serde_json::to_string(&state).unwrap();
        let restored: PersistentState = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.subscriptions[0].group_id, 123_456);
    }

    #[test]
    fn stale_translation_namespaces_are_removed_without_touching_subscriptions() {
        let mut state = PersistentState::default();
        state
            .subscriptions
            .push(Subscription::new(123_456, BranchMode::All));
        state.translations.insert("old-gid".into(), "旧译文".into());
        state
            .translations
            .insert("official-zhs-v1:new-gid".into(), "新译文".into());

        assert!(state.retain_translation_namespace("official-zhs-v1"));
        assert_eq!(state.subscriptions.len(), 1);
        assert_eq!(state.translations.len(), 1);
        assert!(state.translations.contains_key("official-zhs-v1:new-gid"));
    }
}
