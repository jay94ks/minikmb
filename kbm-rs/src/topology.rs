use crate::model::Channel;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsbNode {
    pub id: String,
    pub parent: String,
    pub container: String,
    pub location: Vec<String>,
    pub address: u32,
    pub name: String,
}
fn hub(n: &UsbNode) -> bool {
    let id = n.id.to_ascii_lowercase();
    id.contains("vid_05e3&pid_0610") || id.contains("vid_05e3&pid_0626")
}
fn branch(n: &UsbNode) -> Vec<String> {
    n.location
        .iter()
        .find(|s| s.starts_with("PCIROOT"))
        .map(|s| {
            s.split('#')
                .filter(|s| s.starts_with("USB("))
                .skip(1)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
fn capture(n: &UsbNode) -> bool {
    let id = n.id.to_ascii_lowercase();
    id.contains("vid_345f&pid_2130") || id.contains("vid_534d&pid_2130")
}
pub fn pair(nodes: &[UsbNode], channels: &mut [Channel]) {
    for c in channels {
        c.capture_id = None;
        c.slot = None;
        c.usb_port = 0;
        c.board_id = format!("mcu:{}", c.uid.raw());
        let node = nodes.iter().find(|n| {
            n.id.to_ascii_uppercase()
                .ends_with(&format!("\\{}", c.uid.raw()))
                && n.id.to_ascii_lowercase().contains("vid_cafe&pid_400")
        });
        let Some(node) = node else {
            c.pairing_error = "USB 연결 위치를 확인할 수 없습니다.".into();
            continue;
        };
        c.usb_port = node.address;
        let Some(parent) = nodes
            .iter()
            .find(|n| n.id.eq_ignore_ascii_case(&node.parent) && hub(n))
        else {
            c.pairing_error = "MCU 허브를 확인할 수 없습니다.".into();
            continue;
        };
        c.board_id = parent.id.to_ascii_lowercase();
        let Some((slot, port)) = (match node.address {
            3 => Some((0, 1)),
            4 => Some((1, 2)),
            _ => None,
        }) else {
            c.pairing_error = "MCU가 회로도의 P3/P4 위치와 일치하지 않습니다.".into();
            continue;
        };
        c.slot = Some(slot);
        // A capture that negotiated USB2 is a direct child of the MCU's very
        // same hub. This physical parent + schematic port is stronger evidence
        // than a USB2/USB3 companion-container heuristic and needs no USB3 peer.
        let direct: Vec<_> = nodes
            .iter()
            .filter(|n| {
                capture(n) && n.parent.eq_ignore_ascii_case(&parent.id) && n.address == port
            })
            .collect();
        if !direct.is_empty() {
            if direct.len() == 1 {
                c.capture_id = Some(direct[0].id.clone());
                c.pairing_error.clear();
            } else {
                c.pairing_error =
                    format!("같은 허브 P{port}의 MS2130이 중복되어 연결하지 않았습니다.");
            }
            continue;
        }
        if parent.container.is_empty()
            || parent
                .container
                .chars()
                .filter(|c| c.is_ascii_hexdigit())
                .all(|c| c == '0')
        {
            c.pairing_error = "허브 식별정보가 없습니다.".into();
            continue;
        }
        let peers: Vec<_> = nodes
            .iter()
            .filter(|n| {
                hub(n)
                    && n.container.eq_ignore_ascii_case(&parent.container)
                    && branch(n) == branch(parent)
            })
            .collect();
        let usb2: Vec<_> = peers
            .iter()
            .filter(|n| n.id.to_ascii_lowercase().contains("pid_0610"))
            .collect();
        let usb3: Vec<_> = peers
            .iter()
            .filter(|n| n.id.to_ascii_lowercase().contains("pid_0626"))
            .collect();
        if usb2.len() != 1 || usb3.len() != 1 || !usb2[0].id.eq_ignore_ascii_case(&parent.id) {
            c.pairing_error = "USB2/USB3 허브 대응이 불명확합니다.".into();
            continue;
        }
        let found: Vec<_> = nodes
            .iter()
            .filter(|n| {
                capture(n) && n.parent.eq_ignore_ascii_case(&usb3[0].id) && n.address == port
            })
            .collect();
        if found.len() == 1 {
            c.capture_id = Some(found[0].id.clone());
            c.pairing_error.clear();
        } else {
            c.pairing_error = format!("허브 P{port}의 MS2130을 확인할 수 없습니다.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Uid;
    fn node(id: String, parent: String, address: u32, suffix: &str) -> UsbNode {
        UsbNode {
            id,
            parent,
            address,
            container: "same-generic-container".into(),
            name: "MS2130".into(),
            location: vec![format!("PCIROOT(0)#USBROOT(0)#USB(5){suffix}")],
        }
    }
    #[test]
    fn pairs_multiple_boards_without_using_duplicate_container_alone() {
        let mut nodes = Vec::new();
        let mut channels = Vec::new();
        for board in 1..=2 {
            let u2 = format!("USB\\VID_05E3&PID_0610\\BOARD{board}");
            let u3 = format!("USB\\VID_05E3&PID_0626\\BOARD{board}");
            nodes.push(node(
                u2.clone(),
                "root".into(),
                board,
                &format!("#USB({board})"),
            ));
            nodes.push(node(
                u3.clone(),
                "root".into(),
                board,
                &format!("#USB({board})"),
            ));
            for slot in 0..2 {
                let uid = Uid::from_bytes(&[board as u8 * 10 + slot as u8; 12]).unwrap();
                nodes.push(node(
                    format!("USB\\VID_CAFE&PID_4004\\{}", uid.raw()),
                    u2.clone(),
                    slot + 3,
                    "",
                ));
                nodes.push(node(
                    format!("USB\\VID_345F&PID_2130\\{board}-{slot}"),
                    u3.clone(),
                    slot + 1,
                    "",
                ));
                channels.push(Channel {
                    uid,
                    name: String::new(),
                    port: "not-identity".into(),
                    connected: true,
                    features: 0x77,
                    target_state: 1,
                    firmware: [1, 0, 0],
                    board_id: String::new(),
                    usb_port: 0,
                    slot: None,
                    capture_id: None,
                    pairing_error: String::new(),
                    observed_at: 0,
                });
            }
        }
        pair(&nodes, &mut channels);
        assert!(channels
            .iter()
            .all(|c| c.capture_id.is_some() && c.slot == Some((c.usb_port - 3) as u8)));
        assert_ne!(channels[0].board_id, channels[2].board_id);
        nodes.push(node(
            "USB\\VID_05E3&PID_0626\\AMBIGUOUS".into(),
            "root".into(),
            1,
            "#USB(1)",
        ));
        pair(&nodes, &mut channels);
        assert!(channels[0].capture_id.is_none());
        assert!(channels[2].capture_id.is_some());
    }
    #[test]
    fn usb2_capture_fallback_uses_the_same_physical_board_hub_and_port() {
        let uid = Uid::from_bytes(&[0x12; 12]).unwrap();
        let parent = "USB\\VID_05E3&PID_0610\\BOARD-A";
        let mut hub = node(parent.into(), "upstream".into(), 2, "#USB(2)");
        // Direct parentage does not depend on a shared USB2/USB3 container ID.
        hub.container.clear();
        let mut nodes = vec![
            hub,
            node(
                format!("USB\\VID_CAFE&PID_4004\\{}", uid.raw()),
                parent.into(),
                3,
                "",
            ),
            node(
                "USB\\VID_345F&PID_2130\\USB2-A".into(),
                parent.into(),
                1,
                "",
            ),
            node(
                "USB\\VID_534D&PID_2130\\OTHER-BOARD".into(),
                "USB\\VID_05E3&PID_0610\\BOARD-B".into(),
                1,
                "",
            ),
        ];
        let mut channels = vec![Channel {
            uid,
            name: String::new(),
            port: "not-identity".into(),
            connected: true,
            features: 0x77,
            target_state: 1,
            firmware: [1, 0, 0],
            board_id: String::new(),
            usb_port: 0,
            slot: None,
            capture_id: None,
            pairing_error: String::new(),
            observed_at: 0,
        }];
        pair(&nodes, &mut channels);
        assert_eq!(channels[0].slot, Some(0));
        assert_eq!(
            channels[0].capture_id.as_deref(),
            Some("USB\\VID_345F&PID_2130\\USB2-A")
        );
        assert!(channels[0].pairing_error.is_empty());
        // Two present capture roots at the exact same board port are ambiguous.
        nodes.push(node(
            "USB\\VID_345F&PID_2130\\DUPLICATE".into(),
            parent.into(),
            1,
            "",
        ));
        pair(&nodes, &mut channels);
        assert!(channels[0].capture_id.is_none());
        assert!(!channels[0].pairing_error.is_empty());
        // The right VID/PID/port on a different parent must never be borrowed.
        nodes.retain(|n| !capture(n) || !n.parent.eq_ignore_ascii_case(parent));
        pair(&nodes, &mut channels);
        assert!(channels[0].capture_id.is_none());
    }
}
