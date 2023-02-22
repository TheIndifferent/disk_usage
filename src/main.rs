mod app_state;

use std::{env, thread };
use std::path::PathBuf;
use slint::Weak;
use crate::app_state::AppState;

static STATE: AppState = AppState::new();

fn main() {
    // TODO: implement proper error handling of starting parameters:
    let cwd: PathBuf = env::current_dir()
        .expect("Failed to get cwd");

    let main_window = MainWindow::new();
    main_window.on_requested_exit(|| {
        std::process::exit(0);
    });
    {
        let main_window_weak = main_window.as_weak();
        main_window.on_step_into(move |i: i32| {
            println!("invoked on_step_into");
            let items: Vec<SizeItem> = STATE.step_into(i);
            let very_weak = main_window_weak.unwrap().as_weak();
            update_ui_items(very_weak, items);
        });
    }
    {
        let main_window_weak = main_window.as_weak();
        main_window.on_step_out(move || {
            println!("invoked on_step_out");
            let items: Vec<SizeItem> = STATE.step_out();
            let very_weak = main_window_weak.unwrap().as_weak();
            update_ui_items(very_weak, items);
        });
    }

    let main_window_weak = main_window.as_weak();
    let _scanning_thread = thread::spawn(move || {
        let items: Vec<SizeItem> = STATE.scan_root_from(cwd);
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

slint::slint! {

    import { SizeItem } from "./ui/size-item-struct.slint";
    import { Style } from "./ui/style.slint";
    import { DiskItem } from "./ui/disk-item.slint";
    import { ItemsList } from "./ui/items-list.slint";
    import { ListView } from "std-widgets.slint";

    export { SizeItem }

    MainWindow := Window {
        title: "Disk Usage";
        background: Style.window-background;
        forward-focus: list;

        property<[SizeItem]> items;

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
