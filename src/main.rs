#![windows_subsystem = "windows"]

mod app_state;
mod startup;

use std::thread;
use std::path::PathBuf;
use std::sync::Arc;
use slint::Weak;
use crate::app_state::AppState;

fn main() {
    let target_dir: PathBuf = startup::target_dir();

    let app_state = Arc::new(AppState::new());

    let main_window = MainWindow::new().unwrap();
    main_window.on_requested_exit(|| {
        std::process::exit(0);
    });
    {
        let app_state_clone = Arc::clone(&app_state);
        let main_window_weak = main_window.as_weak();
        main_window.on_step_into(move |i: i32| {
            match app_state_clone.step_into(i) {
                Some(items) => {
                    let very_weak = main_window_weak.unwrap().as_weak();
                    update_ui_items(very_weak, items);
                }
                None => {}
            }
        });
    }
    {
        let app_state_clone = Arc::clone(&app_state);
        let main_window_weak = main_window.as_weak();
        main_window.on_step_out(move || {
            match app_state_clone.step_out() {
                Some((index, items)) => {
                    let very_weak = main_window_weak.unwrap().as_weak();
                    update_ui_items(very_weak, items);
                    let very_weak = main_window_weak.unwrap().as_weak();
                    update_ui_cursor(very_weak, index);
                }
                None => {}
            }
        });
    }

    let app_state_clone = Arc::clone(&app_state);
    let main_window_weak = main_window.as_weak();
    let _scanning_thread = thread::spawn(move || {
        let items: Vec<SizeItem> = app_state_clone.scan_root_from(target_dir);
        update_ui_items(main_window_weak, items);
    });

    main_window.run();
}


fn update_ui_items(weak_window: Weak<MainWindow>, items: Vec<SizeItem>) {
    slint::invoke_from_event_loop(move || {
        let value = std::rc::Rc::new(slint::VecModel::from(items));
        let _ = weak_window
            .unwrap()
            .set_items(value.into());
    })
        .expect("Invocation of UI update failed");
}

fn update_ui_cursor(weak_window: Weak<MainWindow>, index: usize) {
    slint::invoke_from_event_loop(move || {
        let wnd = weak_window.unwrap();
        let _ = wnd.set_cursor(index as i32);
        let _ = wnd.invoke_center_on_index(index as i32);
    })
        .expect("Invocation of UI update failed");
}

slint::slint! {

    import { SizeItem } from "./ui/size-item-struct.slint";
    import { Style } from "./ui/style.slint";
    import { DiskItem } from "./ui/disk-item.slint";
    import { ItemsList } from "./ui/items-list.slint";
    import { ListView } from "std-widgets.slint";

    export { SizeItem }

    component MainWindow inherits Window {
        title: "Disk Usage";
        background: Style.window-background;
        forward-focus: list;

        in property<[SizeItem]> items;
        in-out property <int> cursor <=> list.cursor;

        public function center_on_index(index: int) {
            list.center_on_index(index);
        }

        callback requested_exit <=> list.requested_exit;
        callback step_out <=> list.step_out;
        callback step_into <=> list.step_into;

        Rectangle {
            list := ItemsList {
                items: root.items;
                y: 6pt;
                height: parent.height - 12pt;
                width: parent.width;
                for item[i] in root.items : DiskItem {
                    size_item: item;
                    active: i == list.cursor;
                }
            }
        }
    }
}
