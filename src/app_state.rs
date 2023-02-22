use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::SizeItem;

enum Node {
    File { name: String, size: u64 },
    Dir { name: String, nodes: Vec<Arc<Node>> },
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

struct RootAndNavigation {
    root_node: Arc<Node>,
    navigation: Vec<Arc<Node>>,
}

pub struct AppState {
    state: Mutex<RootAndNavigation>,
}

impl AppState {
    pub fn new() -> AppState {
        AppState {
            state: Mutex::new(
                RootAndNavigation {
                    root_node: Arc::new(Node::Dir { name: "{root node}".to_string(), nodes: Vec::new() }),
                    navigation: Vec::new(),
                }
            ),
        }
    }

    pub fn scan_root_from(&self, path: PathBuf) -> Vec<SizeItem> {
        {
            let node = files::scan_dir_recursive_depth_first(&path);
            let mut state = self.state.lock()
                .expect("Failed to acquire mutex lock on state");
            state.root_node = Arc::new(node);
        }
        return self.root_size_items();
    }

    pub fn step_out(&self) -> Option<Vec<SizeItem>> {
        {
            let mut state = self.state.lock()
                .expect("Failed to acquire mutex lock on state");
            let nav = &mut state.navigation;
            if nav.len() == 0 {
                // we are already on the root node, ignoring:
                return None;
            }
            let nav_len = nav.len();
            nav.remove(nav_len - 1);
            if nav.len() > 0 {
                let node = Arc::clone(&nav[nav.len() - 1]);
                return Some(ui::node_to_size_items(node));
            }
        }
        return Some(self.root_size_items());
    }

    pub fn step_into(&self, index: i32) -> Option<Vec<SizeItem>> {
        // TODO: add support for item 0 being an up folder
        let subnode_result = self.subnode_with_index(index);
        let target_node = match subnode_result {
            Ok(arc) => arc,
            Err(e) => {
                eprintln!("{}", e);
                return Some(self.clear_navigation_and_return_to_root());
            }
        };
        match target_node.as_ref() {
            Node::File { name: _, size: _ } => {
                eprintln!("On step into operation, attempting to step into a file, ignoring.");
                None
            }
            Node::Dir { name: _, nodes } => {
                self.state.lock()
                    .expect("Failed to acquire mutex lock on navigation")
                    .navigation
                    .push(Arc::clone(&target_node));
                let items: Vec<SizeItem> = ui::subnodes_to_size_items(&nodes);
                Some(items)
            }
        }
    }

    fn clear_navigation_and_return_to_root(&self) -> Vec<SizeItem> {
        self.state.lock()
            .expect("Failed to acquire lock for clearing")
            .navigation.clear();
        return self.root_size_items();
    }

    fn root_size_items(&self) -> Vec<SizeItem> {
        ui::node_ref_to_size_items(
            &self.state.lock()
                .expect("Failed to acquire mutex lock on root node").root_node)
    }

    fn subnode_with_index(&self, index: i32) -> Result<Arc<Node>, &str> {
        if index < 0 {
            return Err("On step into operation, attempting to step into element outside of elements size, ignoring.");
        }
        let current_node = self.current_node();
        let subnodes: &Vec<Arc<Node>> = match current_node.as_ref() {
            Node::File { name: _, size: _ } => {
                panic!("On step into operation, current node appears to be a file rather than a dir. App state got corrupted.");
            }
            Node::Dir { name: _, nodes } => &nodes
        };
        if index >= subnodes.len() as i32 {
            return Err("On step into operation, attempting to step into element outside of elements size, ignoring.");
        }
        let selected_node = &subnodes[index as usize];
        Ok(Arc::clone(selected_node))
    }

    fn current_node(&self) -> Arc<Node> {
        let state = self.state.lock()
            .expect("Failed to acquire mutex lock on state");
        let nav = &state.navigation;
        let current_node = if nav.len() == 0 {
            &state.root_node
        } else {
            &nav[nav.len() - 1]
        };
        return Arc::clone(current_node);
    }
}

mod files {
    use std::cmp::Ordering;
    use std::fs::{metadata, read_dir};
    use std::path::PathBuf;
    use std::sync::Arc;
    use super::Node;

    pub(super) fn scan_dir_recursive_depth_first(path: &PathBuf) -> Node {
        if path.is_dir() {
            let reading_dir = read_dir(path);
            match reading_dir {
                Ok(rd) => {
                    let mut nodes: Vec<Arc<Node>> = Vec::new();
                    for entry in rd {
                        match entry {
                            Ok(dir_entry) => {
                                let p = dir_entry.path();
                                match dir_entry.file_type() {
                                    Ok(ft) => {
                                        if ft.is_file() {
                                            let file = Node::File { name: path_file_name(&p), size: path_file_size(&p) };
                                            nodes.push(Arc::new(file));
                                        }
                                        if ft.is_dir() {
                                            let n = scan_dir_recursive_depth_first(&p);
                                            nodes.push(Arc::new(n));
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
    use std::sync::Arc;
    use super::Node;
    use super::SizeItem;

    pub(super) fn node_to_size_items(node: Arc<Node>) -> Vec<SizeItem> {
        return node_ref_to_size_items(&node);
    }

    pub(super) fn node_ref_to_size_items(node: &Node) -> Vec<SizeItem> {
        let subnodes: &Vec<Arc<Node>> = match node {
            Node::File { name, size: _ } => return vec![
                SizeItem { name: name.into(), size_string: node.readable_size().into(), relative_size: 0_f32, is_file: true }],
            Node::Dir { name: _, nodes } => nodes,
        };
        return subnodes_to_size_items(subnodes);
    }

    pub(super) fn subnodes_to_size_items(subnodes: &Vec<Arc<Node>>) -> Vec<SizeItem> {
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
                Node::Dir { name: _, nodes: _ } => false,
                Node::File { name: _, size: _ } => true
            },
        }
    }
}
