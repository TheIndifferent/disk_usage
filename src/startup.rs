use std::env;
use std::path::PathBuf;

pub fn target_dir() -> PathBuf {
    path_or_error_dialog(
        determine_root_directory())
}

fn determine_root_directory() -> Result<PathBuf, ErrMessage> {
    let arg = env::args().nth(1);
    match arg {
        Some(a) => {
            let path = PathBuf::from(&a);
            if !path.exists() {
                return Err(ErrMessage {
                    message: "Path does not exist:".into(),
                    path: a.into()
                });
            }
            if !path.is_dir() {
                return Err(ErrMessage {
                    message: "Path is not a directory:".into(),
                    path: a.into()
                });
            }
            return Ok(path);
        }
        None => {}
    }
    env::current_dir()
        .map_err(|_e| ErrMessage {
            message: "Cannot determine current directory".into(),
            path: "".into()
        })
}

fn path_or_error_dialog(desired_target: Result<PathBuf, ErrMessage>) -> PathBuf {
    match desired_target {
        Ok(path) => return path,
        Err(e) => {
            let dialog = ErrorDialog::new().unwrap();
            dialog.invoke_set_message(e.into());
            dialog.on_close_confirmed(|| {
                std::process::exit(1);
            });
            dialog.run();
        }
    }
    std::process::exit(1);
}

slint::slint! {

    import { Style } from "./ui/style.slint";
    import { StandardButton } from "std-widgets.slint";

    struct ErrMessage {
        message: string,
        path: string,
    }

    export { ErrMessage }

    component ErrorDialog inherits Dialog {

        background: #1f1f1f;

        public function set_message(s: ErrMessage) {
            message.text = s.message;
            path.text = s.path;
        }
        callback close_confirmed;

        forward-focus: fs;

        Rectangle {
            height: 60pt;
            width: path.width + 20pt;
            message := Text {
                x: 6pt;
                y: 6pt;
                color: Style.text-main;
                font-family: "Segoe UI";
                font-size: 12pt;
            }
            path := Text {
                x: 6pt;
                y: 32pt;
                color: Style.text-main;
                font-family: "Consolas";
                font-size: 12pt;
            }
            fs := FocusScope {
                key-pressed(event) => {
                    if (event.text == Key.Escape || event.text == Key.Return) {
                        root.close_confirmed();
                        return accept;
                    }
                    return reject;
                }
            }
        }
        StandardButton {
            height: 25pt;
            //color: Style.list-item-background;
            kind: close;
            clicked => { root.close_confirmed(); }
        }
    }
}
