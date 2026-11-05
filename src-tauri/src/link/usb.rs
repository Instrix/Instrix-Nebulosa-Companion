//! NBP over USB: the vendor interface (class FF / 4E / 01). Its OUT
//! endpoint takes commands; every IN endpoint is read (High-Speed: EP1 DATA,
//! EP2 responses; Full-Speed: EP3 both), each as its own byte stream.

use super::{Input, Writer};
use nbp_core::msg;
use nusb::transfer::{EndpointType, RequestBuffer};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const IN_TRANSFER: usize = 16 * 1024;
const IN_QUEUE: usize = 8;

#[derive(Serialize)]
pub struct UsbDev {
    pub id: String,
    pub label: String,
    pub serial: Option<String>,
    pub speed: Option<String>,
}

fn is_nbp(d: &nusb::DeviceInfo) -> bool {
    d.vendor_id() == msg::USB_VID && (d.product_id() == msg::USB_PID_HS || d.product_id() == msg::USB_PID_FS)
}

fn dev_id(d: &nusb::DeviceInfo) -> String {
    format!("{}:{}", d.bus_number(), d.device_address())
}

pub fn list() -> Result<Vec<UsbDev>, String> {
    Ok(nusb::list_devices()
        .map_err(|e| e.to_string())?
        .filter(is_nbp)
        .map(|d| UsbDev {
            id: dev_id(&d),
            label: format!(
                "{} {} ({})",
                d.manufacturer_string().unwrap_or("INSTRIX"),
                d.product_string().unwrap_or("Nebulosa"),
                if d.product_id() == msg::USB_PID_FS { "USB-C, Full-Speed" } else { "USB 3 port" },
            ),
            serial: d.serial_number().map(str::to_owned),
            speed: d.speed().map(|s| format!("{s:?}")),
        })
        .collect())
}

pub(crate) async fn open(
    id: &str,
    tx: mpsc::Sender<Input>,
) -> Result<(Writer, Vec<JoinHandle<()>>, String), String> {
    let info = nusb::list_devices()
        .map_err(|e| e.to_string())?
        .find(|d| is_nbp(d) && dev_id(d) == id)
        .ok_or("device gone; refresh the list")?;
    let dev = info.open().map_err(|e| no_driver(format!("open: {e}")).unwrap_or_else(|e| if cfg!(windows) { e } else { format!("{e} (permissions? see udev/)") }))?;
    let cfg = dev.active_configuration().map_err(|e| e.to_string())?;
    let alt = cfg
        .interface_alt_settings()
        .find(|a| a.class() == msg::USB_CLASS && a.subclass() == msg::USB_SUBCLASS && a.protocol() == msg::USB_PROTOCOL)
        .ok_or("no NBP interface on this device")?;
    let bulk: Vec<u8> = alt
        .endpoints()
        .filter(|e| e.transfer_type() == EndpointType::Bulk)
        .map(|e| e.address())
        .collect();
    let out_ep = *bulk.iter().find(|&&a| a & 0x80 == 0).ok_or("no OUT endpoint")?;
    let in_eps: Vec<u8> = bulk.iter().copied().filter(|&a| a & 0x80 != 0).collect();
    if in_eps.is_empty() {
        return Err("no IN endpoint".into());
    }
    let iface = dev
        .claim_interface(alt.interface_number())
        .map_err(|e| no_driver(format!("claim: {e}")).unwrap_or_else(|e| e))?;

    let tasks = in_eps
        .iter()
        .enumerate()
        .map(|(src, &ep)| {
            let mut q = iface.bulk_in_queue(ep);
            let tx = tx.clone();
            tokio::spawn(async move {
                for _ in 0..IN_QUEUE {
                    q.submit(RequestBuffer::new(IN_TRANSFER));
                }
                loop {
                    let c = q.next_complete().await;
                    if let Err(e) = c.status {
                        let _ = tx.send(Input::Closed(format!("USB IN 0x{ep:02X}: {e}"))).await;
                        break;
                    }
                    if !c.data.is_empty() && tx.send(Input::Bytes(src, c.data.clone())).await.is_err() {
                        break;
                    }
                    q.submit(RequestBuffer::reuse(c.data, IN_TRANSFER));
                }
            })
        })
        .collect();

    let speed = info.speed().map(|s| format!(" {s:?}")).unwrap_or_default();
    Ok((Writer::Usb { iface, ep: out_ep }, tasks, format!("USB {id}{speed}")))
}

/// Windows with no WinUSB driver on the board (firmware without MS OS 2.0
/// descriptors): say what to do, not the USB stack's words.
fn no_driver(e: String) -> Result<String, String> {
    if cfg!(windows) && (e.contains("determine driver") || e.contains("not supported") || e.contains("driver")) {
        Ok(format!(
            "Windows has no driver for the board yet. Once per PC: run Zadig (zadig.akeo.ie), \
             Options › List All Devices, pick Nebulosa, driver WinUSB, Install Driver; then connect again. ({e})"
        ))
    } else {
        Err(e)
    }
}

