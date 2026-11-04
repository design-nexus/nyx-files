// SPDX-License-Identifier: MIT

use super::*;




#[test]
fn paste_resolution_does_not_change_input_ownership() {
    let mut input = InputOwnership::default();
    for method in [NavigationInput::Pointer, NavigationInput::Keyboard] {
        input.last_navigation = method;
        for _ in 0..3 {
            assert_eq!(
                input.destination(Some(0), Some(1), Some(2), 3),
                Some(if method == NavigationInput::Pointer {
                    0
                } else {
                    1
                })
            );
            assert_eq!(input.last_navigation, method);
        }
    }
}

#[test]
fn keyboard_targets_focused_parent_not_deepest_open_column() {
    let input = InputOwnership::default();
    assert_eq!(input.destination(None, Some(0), Some(2), 3), Some(0));
    assert_eq!(input.destination(None, None, Some(1), 3), Some(1));
}

