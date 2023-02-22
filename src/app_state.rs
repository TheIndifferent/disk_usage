use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Mutex;

use super::SizeItem;

enum Node {
    Root { nodes: Vec<Node> },
    File { name: String, size: u64 },
    Dir { name: String, nodes: Vec<Node> },
}

impl Node {
    fn name(&self) -> String {
        match &self {
            Node::Root { nodes: _ } => "{root node}".to_string(),
            Node::File { name, size: _ } => name.to_string(),
            Node::Dir { name, nodes: _ } => name.to_string()
        }
    }

    fn size(&self) -> u64 {
        match &self {
            Node::Root { nodes } => nodes.iter().map(|n| n.size()).sum(),
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

pub struct AppState {
    root_node: Mutex<Cell<Node>>,
    navigation: Mutex<Vec<&'static Node>>,
}

impl AppState {
    pub const fn new() -> AppState {
        AppState {
            root_node: Mutex::new(Cell::new(Node::Root { nodes: Vec::new() })),
            navigation: Mutex::new(Vec::new()),
        }
    }

    pub fn scan_root_from(&self, path: PathBuf) -> Vec<SizeItem> {
        {
            let node = files::scan_dir_recursive_depth_first(&path);
            let mut cell = self.root_node.lock()
                .expect("Failed to acquire mutex lock on root node");
            cell.set(node);
        }
        return self.root_size_items();
    }

    pub fn step_out(&self) -> Vec<SizeItem> {
        let mut nav = self.navigation.lock()
            .expect("Failed to acquire mutex lock on navigation");
        if nav.len() == 0 {
            // we are on the root node, return root node contents:
            return self.root_size_items();
        }
        let nav_len = nav.len();
        nav.remove(nav_len - 1);
        if nav.len() == 0 {
            return self.root_size_items();
        }
        let node: &Node = nav[nav.len() - 1];
        return ui::node_to_size_items(node);
    }

    pub fn step_into(&self, index: i32) -> Vec<SizeItem> {
        // TODO: add support for item 0 being an up folder
        let mut nav = self.navigation.lock()
            .expect("Failed to acquire mutex lock on navigation");
        let mut root = self.root_node.lock()
            .expect("Failed to acquire mutex lock on root node");
        let current_node: &Node = if nav.len() == 0 {
            root.get_mut()
        } else {
            nav[nav.len() - 1]
        };
        let subnodes: &Vec<Node> = match current_node {
            Node::File { name: _, size: _ } => {
                panic!("On step into operation, current node appears to be a file rather than a dir. App state got corrupted.");
            },
            Node::Root { nodes } => nodes,
            Node::Dir { name: _, nodes } => nodes
        };
        if index < 0 || index >= subnodes.len() as i32 {
            eprintln!("On step into operation, attempting to step into element outside of elements size, ignoring.");
            return ui::node_to_size_items(current_node);
        }
        let target_node: &Node = &subnodes[index as usize];
        match target_node {
            Node::File { name: _, size: _ } => {
                eprintln!("On step into operation, attempting to step into a file, ignoring.");
                return ui::node_to_size_items(current_node);
            },
            Node::Root { nodes: _ } => {
                return self.root_size_items();
            },
            Node::Dir { name: _, nodes } => {
                // self.navigation.lock()
                //     .expect("Failed to acquire mutex lock on navigation")
                //     .push(target_node);
                // nav.push(target_node);
                let items: Vec<SizeItem> = ui::subnodes_to_size_items(nodes);
                return items;
            }
        }
    }

    fn root_size_items(&self) -> Vec<SizeItem> {
        ui::node_to_size_items(
            self.root_node.lock()
                .expect("Failed to acquire mutex lock on root node")
                .get_mut())
    }
}

mod files {
    use std::cmp::Ordering;
    use std::fs::{metadata, read_dir};
    use std::path::PathBuf;
    use super::Node;

    pub(super) fn scan_dir_recursive_depth_first(path: &PathBuf) -> Node {
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
}

mod ui {
    use super::Node;
    use super::SizeItem;

    pub(super) fn node_to_size_items(node: &Node) -> Vec<SizeItem> {
        let subnodes: &Vec<Node> = match node {
            Node::File { name, size: _ } => return vec![
                SizeItem { name: name.into(), size_string: node.readable_size().into(), relative_size: 0_f32, is_file: true }],
            Node::Dir { name: _, nodes } => nodes,
            Node::Root { nodes } => nodes
        };
        return subnodes_to_size_items(subnodes);
    }

    pub(super) fn subnodes_to_size_items(subnodes: &Vec<Node>) -> Vec<SizeItem> {
        let max_size = subnodes.iter().map(|i| i.size()).max().unwrap_or(0);
        return subnodes.iter()
            .map(|node| node_to_size_item(node, &max_size))
            .collect();
    }

    fn node_to_size_item(node: &Node, max_size: &u64) -> SizeItem {
        SizeItem {
            name: node.name().into(),
            size_string: node.readable_size().into(),
            relative_size: (node.size() as f64 / *max_size as f64) as f32,
            is_file: match node {
                Node::Root { nodes: _ } => false,
                Node::Dir { name: _, nodes: _ } => false,
                Node::File { name: _, size: _ } => true
            },
        }
    }
}
