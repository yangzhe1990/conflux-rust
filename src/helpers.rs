use dir::helpers::replace_home;

pub fn conflux_ipc_path(base: &str, path: &str, shift: u16) -> String {
    let mut path = path.to_owned();
    if shift != 0 {
        path = path.replace("jsonrpc.ipc", &format!("jsonrpc-{}.ipc", shift))
    }
    replace_home(base, &path)
}
