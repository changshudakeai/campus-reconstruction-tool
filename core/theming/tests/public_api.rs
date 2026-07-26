//! 公开 API 快照测试 (执法清单 2.5)

#[test]
fn public_api_types_exist() {
    // ColorRole enum exists and can be constructed
    let _role = theming::ColorRole::TextPrimary;
    
    // ColorCard struct exists with required methods
    let mut colors = std::collections::HashMap::new();
    colors.insert(theming::ColorRole::PrimaryBackground, "#FFFFFF".to_string());
    let card = theming::ColorCard { name: "Test".into(), colors };
    assert_eq!(card.get(theming::ColorRole::PrimaryBackground), Some("#FFFFFF"));
    
    // MotionToken enum
    let _fast = theming::MotionToken::Fast;
    let _medium = theming::MotionToken::Medium;
    
    // MotionTable can be constructed
    let table = theming::MotionTable {
        fast: theming::DurationConfig { duration: 0.2, easing: theming::EasingType::Linear },
        medium: theming::DurationConfig { duration: 0.5, easing: theming::EasingType::EaseOut },
        slow: theming::DurationConfig { duration: 1.0, easing: theming::EasingType::EaseInOut },
    };
    assert_eq!(table.get_duration(theming::MotionToken::Fast), 0.2);
    
    // ThemeMode enum
    let _light = theming::ThemeMode::Light;
    let _dark = theming::ThemeMode::Dark;
    let _system = theming::ThemeMode::System;
    assert_eq!(_light.display_name(), "亮色");
    
    // ThemeManager built_in constructor
    let manager = theming::ThemeManager::built_in();
    assert_eq!(manager.current_mode(), theming::ThemeMode::Light);
    
    // Check that theme assets loaded successfully
    assert!(manager.active_colors().get(theming::ColorRole::PrimaryBackground).is_some());
    
    // Motion settings
    assert!(manager.has_any_animation());
}
