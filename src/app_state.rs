use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::SizeItem;

enum Node {
    File { name: String, size_on_disk: u64, size_real: u64 },
    Dir { name: String, nodes: Vec<Arc<Node>> },
}

impl Node {
    fn name(&self) -> String {
        match &self {
            Node::File { name, size_on_disk: _, size_real: _ } => name.to_string(),
            Node::Dir { name, nodes: _ } => name.to_string()
        }
    }

    fn size_real(&self) -> u64 {
        match &self {
            Node::File { name: _, size_on_disk: _, size_real } => *size_real,
            Node::Dir { name: _, nodes } => nodes.iter().map(|n| n.size_real()).sum()
        }
    }

    fn size_on_disk(&self) -> u64 {
        match &self {
            Node::File { name: _, size_on_disk, size_real: _ } => *size_on_disk,
            Node::Dir { name: _, nodes } => nodes.iter().map(|n| n.size_on_disk()).sum()
        }
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
            let cluster_size = files::cluster_size_for_path(&path);
            let node = files::scan_dir_recursive_depth_first(&path, &cluster_size);
            let mut state = self.state.lock()
                .expect("Failed to acquire mutex lock on state");
            state.root_node = Arc::new(node);
        }
        return self.root_size_items();
    }

    pub fn step_out(&self) -> Option<(usize, Vec<SizeItem>)> {
        let mut state = self.state.lock()
            .expect("Failed to acquire mutex lock on state");
        let nav = &mut state.navigation;
        if nav.len() == 0 {
            // we are already on the root node, ignoring:
            return None;
        }
        let nav_len = nav.len();
        let current = nav.remove(nav_len - 1);
        let one_up: Arc<Node> = if nav.len() > 0 {
            Arc::clone(&nav[nav.len() - 1])
        } else {
            Arc::clone(&state.root_node)
        };
        let index = self.index_of_subnode_in_node(&current, &one_up);
        return Some((index, ui::node_to_size_items(one_up)));
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
            Node::File { name: _, size_on_disk: _, size_real: _ } => {
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
            Node::File { name: _, size_on_disk: _, size_real: _ } => {
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

    fn index_of_subnode_in_node(&self, subnode: &Node, node: &Node) -> usize {
        let subnodes = match node {
            Node::File { name: _, size_on_disk: _, size_real: _ } => return 0,
            Node::Dir { name: _, nodes } => nodes,
        };
        let mut index: usize = 0;
        for n in subnodes {
            let nn: &Node = n;
            // TODO implement Eq or something to make this check cleaner:
            if subnode.name() == nn.name()
                && subnode.size_real() == nn.size_real() {
                return index;
            }
            index = index + 1;
        }
        return 0;
    }
}

mod files {
    use std::cmp::Ordering;
    use std::fs::{metadata, read_dir};
    use std::path::PathBuf;
    use std::sync::Arc;
    use windows::core::{HSTRING};
    use windows::Win32::Storage::FileSystem::{GetVolumePathNameW, GetDiskFreeSpaceW };
    use super::Node;

    pub(super) fn cluster_size_for_path(path: &PathBuf) -> u64 {
        let root_path: &mut [u16] = &mut [0; 261];
        let path_result = unsafe { GetVolumePathNameW(&HSTRING::from(path.as_path()), root_path) };
        if !path_result.as_bool() {
            // TODO: make it Result instead
            panic!("Could not determine volumen name for path");
        }
        let mut sectors_per_cluster: u32 = 0;
        let mut bytes_per_sector: u32 = 0;
        let root_disk = HSTRING::from_wide(root_path).expect("Failed to convert [u16] into HSTRING");
        let disk_free_result = unsafe { GetDiskFreeSpaceW(&root_disk, Some(&mut sectors_per_cluster), Some(&mut bytes_per_sector), None, None) };
        if !disk_free_result.as_bool() {
            // TODO: make it Result instead
            panic!("Could not determine cluster size for disk");
        }
        u64::from(sectors_per_cluster) * u64::from(bytes_per_sector)
    }

    pub(super) fn scan_dir_recursive_depth_first(path: &PathBuf, cluster_size: &u64) -> Node {
        if path.is_dir() {
            let reading_dir = read_dir(path);
            match reading_dir {
                Ok(rd) => {
                    let mut nodes: Vec<Arc<Node>> = Vec::new();
                    for entry in rd {
                        match entry {
                            Ok(dir_entry) => {
                                let p = dir_entry.path();
                                nodes.push(Arc::new(scan_dir_recursive_depth_first(&p, cluster_size)));
                            }
                            Err(e) => {
                                eprintln!("Failed to process dir entry because of: {:?}", e);
                            }
                        }
                    }
                    nodes.sort_by(|a, b| {
                        let diff = a.size_on_disk() as i128 - b.size_on_disk() as i128;
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
                    return Node::File {
                        name: path_file_name(path),
                        size_on_disk: 0,
                        size_real: 0,
                    };
                }
            }
        }
        if path.is_file() {
            let name = path_file_name(path);
            let size = path_file_size(path);
            let size_on_disk = ((size + cluster_size - 1) / cluster_size) * cluster_size;
            return Node::File {
                name: name,
                size_on_disk: size_on_disk,
                size_real: size,
            };
        }
        Node::File {
            name: path_file_name(path),
            size_on_disk: 0,
            size_real: 0,
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
            Node::File { name, size_on_disk: _, size_real } => return vec![
                SizeItem {
                    name: name.into(),
                    size_string: readable_size(size_real).into(),
                    relative_real_size: 1_f32,
                    relative_disk_size: 1_f32,
                    is_file: true,
                }],
            Node::Dir { name: _, nodes } => nodes,
        };
        return subnodes_to_size_items(subnodes);
    }

    pub(super) fn subnodes_to_size_items(subnodes: &Vec<Arc<Node>>) -> Vec<SizeItem> {
        // using 1 as default to avoid division by 0:
        let max_disk_size = subnodes.iter().map(|i| i.size_on_disk()).max().unwrap_or(1);
        let max_real_size = subnodes.iter().map(|i| i.size_real()).max().unwrap_or(1);
        return subnodes.iter()
            .map(|node| node_to_size_item(node, &max_real_size, &max_disk_size))
            .collect();
    }

    fn node_to_size_item(node: &Node, max_real_size: &u64, max_disk_size: &u64) -> SizeItem {
        let size_real = node.size_real();
        let size_on_disk = node.size_on_disk();
        let readable_size = format!("{} ({} on disk)", readable_size(&size_real), readable_size(&size_on_disk));
        SizeItem {
            name: node.name().into(),
            size_string: readable_size.into(),
            relative_real_size: (size_real as f64 / *max_real_size as f64) as f32,
            relative_disk_size: (size_on_disk as f64 / *max_disk_size as f64) as f32,
            is_file: match node {
                Node::Dir { name: _, nodes: _ } => false,
                Node::File { name: _, size_on_disk: _, size_real: _ } => true
            },
        }
    }

    fn readable_size(input_size: &u64) -> String {
        let mut size: u64 = *input_size;
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
                }
                size_remainder = size_remainder / 10;
                if size_remainder < 10 {
                    return format!("{}.0{} {}", size, size_remainder, unit);
                }
                return format!("{}.{} {}", size, size_remainder, unit);
            }
            size_remainder = size % 1000;
            size = size / 1000;
        }
        eprintln!("overflown any reasonable units: {}", input_size);
        return format!("{} {}", input_size, "B");
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn correct_kilobytes_of_sector() {
            assert_eq!("4.09 kB", readable_size(&4096));
        }

        #[test]
        fn correct_number_of_digits() {
            assert_eq!("6.72 kB", readable_size(&6725));
        }
    }
}
