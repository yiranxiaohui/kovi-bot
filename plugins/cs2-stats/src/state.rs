use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct State {
    pub bindings: BTreeMap<i64, String>,
}

impl State {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            let state = Self::default();
            state.save(path)?;
            return Ok(state);
        }
        let contents =
            std::fs::read_to_string(path).map_err(|e| format!("读取绑定数据失败: {e}"))?;
        serde_json::from_str(&contents).map_err(|e| format!("解析绑定数据失败: {e}"))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| "绑定数据路径无效".to_string())?;
        std::fs::create_dir_all(parent).map_err(|e| format!("创建插件目录失败: {e}"))?;
        let temporary = path.with_extension("json.tmp");
        let contents =
            serde_json::to_vec_pretty(self).map_err(|e| format!("序列化绑定数据失败: {e}"))?;
        std::fs::write(&temporary, contents).map_err(|e| format!("写入绑定数据失败: {e}"))?;
        std::fs::rename(&temporary, path).map_err(|e| format!("保存绑定数据失败: {e}"))
    }
}
