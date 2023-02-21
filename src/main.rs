use std::cmp::Ordering;
use std::{env, thread, time};
use std::fs::{metadata, read_dir};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

fn main() {
    // TODO: add error dialogs in case root is not a folder
    match env::current_dir() {
        Ok(cwd) => scan_dir_from(cwd),
        Err(_e) => eprintln!("Failed to get current directory")
    }
}

fn scan_dir_from(root: PathBuf) {
    let navigation: Arc<RwLock<Vec<Node>>> = Arc::new(RwLock::new(vec![]));

    let main_window = MainWindow::new();
    main_window.on_requested_exit(|| {
        std::process::exit(0);
    });
    {
        let nav_clone = Arc::clone(&navigation);
        main_window.on_step_into(|i: i32| {});
    }
    {
        let nav_clone = Arc::clone(&navigation);
        main_window.on_step_out(move || {
            match nav_clone.read() {
                Ok(nav) => {
                    if nav.len() == 1_usize {
                        return;
                    }
                }
                Err(e) => {
                    eprintln!("Failed to acquire read lock: {:?}", e);
                }
            }
        });
    }

    let nav_clone = Arc::clone(&navigation);
    let main_window_weak = main_window.as_weak();
    let _scanning_thread = thread::spawn(move || {
        let root_node = scan_dir_recursive_depth_first(&root);
        match nav_clone.write() {
            Ok(mut nav) => {
                nav.push(root_node);
            }
            Err(e) => {
                eprintln!("Failed to acquire write lock: {:?}", e);
            }
        }
        match nav_clone.read() {
            Ok(nav) => {
                let root_node_reference = &nav[0];
                let items: Vec<SizeItem> = node_to_size_items(root_node_reference);
                slint::invoke_from_event_loop(move || {
                    let value = std::rc::Rc::new(slint::VecModel::from(items));
                    main_window_weak
                        .unwrap()
                        .set_items(value.into());
                })
                    .expect("Invocation of UI update failed");
            }
            Err(e) => {
                eprintln!("Failed to acquire read lock after change: {:?}", e);
            }
        }
    });

    main_window.run();
}

fn scan_dir_recursive_depth_first(path: &PathBuf) -> Node {
    if path.is_dir() {
        let reading_dir = read_dir(path);
        match reading_dir {
            Ok(rd) => {
                let mut nodes: Vec<Node> = Vec::new();
                for entry in rd {
                    match entry {
                        Ok(dir_entry) => {
                            let p = dir_entry.path();
                            match dir_entry.file_type() {
                                Ok(ft) => {
                                    if ft.is_file() {
                                        let file = Node::File { name: path_file_name(&p), size: path_file_size(&p) };
                                        nodes.push(file);
                                    }
                                    if ft.is_dir() {
                                        let n = scan_dir_recursive_depth_first(&p);
                                        nodes.push(n);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Failed to determine file type because of: {:?}", e)
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to process dir entry because of: {:?}", e);
                        }
                    }
                }
                nodes.sort_by(|a, b| {
                    let diff = a.size() as i128 - b.size() as i128;
                    if diff < 0 {
                        return Ordering::Greater;
                    }
                    if diff > 0 {
                        return Ordering::Less;
                    }
                    Ordering::Equal
                });
                return Node::Dir { name: path_file_name(path), nodes };
            }
            Err(e) => {
                eprintln!("Failed to read dir: {:?}, because of: {:?}", path, e);
                return Node::File { name: path_file_name(path), size: 0 };
            }
        }
    }
    if path.is_file() {
        return Node::File {
            name: path_file_name(path),
            size: path_file_size(path),
        };
    }
    Node::File {
        name: path_file_name(path),
        size: 0,
    }
}

fn path_file_name(path: &PathBuf) -> String {
    path.file_name()
        .map(|s| s.to_os_string())
        .and_then(|s| {
            match s.into_string() {
                Ok(o) => Some(o),
                Err(e) => {
                    eprintln!("Failed to convert OsString into String: {:?}", e);
                    None
                }
            }
        })
        .unwrap_or("<invalid name>".to_string())
}

fn path_file_size(path: &PathBuf) -> u64 {
    let s = metadata(path).map(|md| md.len());
    match s {
        Ok(size) => size,
        Err(e) => {
            eprintln!("Failed to read size of the file: {:?} because of {:?}", path, e);
            0
        }
    }
}

fn node_to_size_items(node: &Node) -> Vec<SizeItem> {
    match node {
        Node::File { name, size: _ } => vec![SizeItem { name: name.into(), size_string: node.readable_size().into(), relative_size: 0_f32, is_file: true }],
        Node::Dir { name: _, nodes } => {
            let max_size = nodes.iter().map(|i| i.size()).max().unwrap_or(0);
            nodes.iter()
                .map(|node| node_to_size_item(node, &max_size))
                .collect()
        }
    }
}

fn node_to_size_item(node: &Node, max_size: &u64) -> SizeItem {
    SizeItem {
        name: node.name().into(),
        size_string: node.readable_size().into(),
        relative_size: (node.size() as f64 / *max_size as f64) as f32,
        is_file: match node {
            Node::Dir { name: _, nodes: _ } => false,
            Node::File { name: _, size: _ } => true
        },
    }
}

enum Node {
    File { name: String, size: u64 },
    Dir { name: String, nodes: Vec<Node> },
}

impl Node {
    fn name(&self) -> String {
        match &self {
            Node::File { name, size: _ } => name.to_string(),
            Node::Dir { name, nodes: _ } => name.to_string()
        }
    }

    fn size(&self) -> u64 {
        match &self {
            Node::File { name: _, size } => *size,
            Node::Dir { name: _, nodes } => nodes.iter().map(|n| n.size()).sum()
        }
    }

    fn readable_size(&self) -> String {
        let mut size: u64 = self.size();
        let mut size_remainder: u64 = 0;
        let units = vec!["B", "kB", "MB", "GB", "TB", "PB"];
        for unit in units {
            if size < 1000 {
                // keep size at 3 digits:
                if size >= 100 {
                    return format!("{} {}", size, unit);
                }
                if size >= 10 {
                    if size_remainder / 100 == 0 {
                        return format!("{} {}", size, unit);
                    } else {
                        return format!("{}.{} {}", size, size_remainder / 100, unit);
                    }
                }
                if size_remainder / 10 == 0 {
                    return format!("{} {}", size, unit);
                } else {
                    return format!("{}.{} {}", size, size_remainder / 10, unit);
                }
            }
            size_remainder = size % 1000;
            size = size / 1000;
        }
        eprintln!("overflown any reasonable units: {}", self.size());
        return format!("{} {}", self.size(), "B");
        // TODO: use float formatting but strip trailing zeros:
        // let mut size: f64 = self.size() as f64;
        // let units = vec!["B", "kB", "MB", "GB", "TB"];
        // let mut iteration = 0;
        // loop {
        //     if size < 1000_f64 {
        //         return format!("{:.2}{}", size, units[iteration]);
        //     }
        //     size = size / 1000_f64;
        //     iteration = iteration + 1;
        // }
    }
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
        callback step_out;
        callback step_into(int);

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
