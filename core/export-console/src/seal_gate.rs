//! 封账门控接缝（缝 5 的"闸"，窗口契约章）
//!
//! F* 功能模块横向零依赖（ADR-0017）：F9 不 import F5 类型，封账动作经
//! [`SealGate`] trait 由壳接线——壳的实现内部调用 F5
//! `ReviewWorkbench::seal()` 批量写回，失败时封账不生效（缝 4 契约）。
//!
//! ## 回滚语义（ADR-0022 + 缝 5）
//!
//! - [`SealGate::seal`]：确认即封账，成功后评审入口禁用；
//! - [`SealGate::release`]：导出失败时释放封账——壳丢弃已封账的评审台
//!   实例，下次进台从 B2 重新读入（新实例未封账，评审恢复可改）。

use shared_domain_types::PlanId;

/// 封账门控 trait（壳实现，F9 只调接口）。
///
/// 错误以 `String` 递回（开发者诊断信息）；F9 把它包进
/// [`Error::SealFailed`](crate::Error::SealFailed) 向上传递，
/// 按弹窗铁律由 B7 呈现。
pub trait SealGate: Send + Sync {
    /// 封账：把评审终态批量写回 B2，成功后评审不可再改。
    ///
    /// 失败时必须保证封账**不生效**（评审保持可改）——
    /// 不出现"账封了但没存上"的半截状态。
    fn seal(&self, plan_id: &PlanId) -> std::result::Result<(), String>;

    /// 释放封账：导出失败回滚时调用，评审恢复可改状态。
    fn release(&self, plan_id: &PlanId) -> std::result::Result<(), String>;
}

/// Mock 门控：内存布尔位模拟封账/解封（单元测试与文档示例用）
#[derive(Debug, Default, Clone)]
pub struct MockSealGate {
    sealed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl MockSealGate {
    /// 创建（初始未封账）
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前是否处于封账状态
    pub fn is_sealed(&self) -> bool {
        self.sealed.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl SealGate for MockSealGate {
    fn seal(&self, _plan_id: &PlanId) -> std::result::Result<(), String> {
        let was_sealed = self.sealed.swap(true, std::sync::atomic::Ordering::SeqCst);
        if was_sealed {
            return Err("重复封账：评审已处于封账状态".to_owned());
        }
        Ok(())
    }

    fn release(&self, _plan_id: &PlanId) -> std::result::Result<(), String> {
        self.sealed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_gate_seals_once_and_releases() {
        let gate = MockSealGate::new();
        let plan_id = PlanId::generate();

        assert!(!gate.is_sealed());
        gate.seal(&plan_id).unwrap();
        assert!(gate.is_sealed());
        // 重复封账被拒绝
        assert!(gate.seal(&plan_id).is_err());
        // 解封后可再次封账
        gate.release(&plan_id).unwrap();
        assert!(!gate.is_sealed());
        gate.seal(&plan_id).unwrap();
        assert!(gate.is_sealed());
    }

    #[test]
    fn clones_share_seal_state() {
        let gate = MockSealGate::new();
        let alias = gate.clone();
        gate.seal(&PlanId::generate()).unwrap();
        assert!(alias.is_sealed());
    }
}
