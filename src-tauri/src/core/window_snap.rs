use tauri::{PhysicalPosition, WebviewWindow};

#[tauri::command]
pub fn snap_window_if_needed(window: WebviewWindow) {
    if let Ok(position) = window.outer_position() {
        if let Some(current_monitor) = window.current_monitor().ok().flatten() {
            let screen_size = current_monitor.size();
            let screen_width = screen_size.width as i32;
            let screen_height = screen_size.height as i32;
            let screen_position = current_monitor.position();
            let window_size = window.inner_size().unwrap();
            let window_width = window_size.width as i32;
            let window_height = window_size.height as i32;
            let mut new_x = position.x;
            let mut new_y = position.y;
            let snap_threshold = screen_width.min(screen_height) / 13;
            const SCREEN_MARGIN: i32 = 0;

            let relative_x = position.x - screen_position.x;
            let relative_y = position.y - screen_position.y;

            if relative_x.abs() < snap_threshold || relative_x < 0 {
                new_x = screen_position.x + SCREEN_MARGIN;
            } else if (screen_width - (relative_x + window_width)).abs() < snap_threshold
                || (relative_x + window_width) > screen_width
            {
                new_x = screen_position.x + screen_width - window_width - SCREEN_MARGIN;
            }
            if relative_y.abs() < snap_threshold || relative_y < 0 {
                new_y = screen_position.y + SCREEN_MARGIN;
            } else if (screen_height - (relative_y + window_height)).abs() < snap_threshold
                || (relative_y + window_height) > screen_height
            {
                new_y = screen_position.y + screen_height - window_height - SCREEN_MARGIN;
            }
            if new_x != position.x || new_y != position.y {
                window
                    .set_position(tauri::Position::Physical(PhysicalPosition {
                        x: new_x,
                        y: new_y,
                    }))
                    .unwrap();
            }
        }
    }
}
