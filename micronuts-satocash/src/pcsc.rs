//! PC/SC transport (feature `pcsc`): talk to a SatoCash card through any
//! pcscd reader — including the micronuts bench nucula CCID/vpcd readers
//! served by ccid-firmware-rs.

use crate::{CardTransport, Error};

/// SatoCash APDU size cap; the applet's status responses fit comfortably.
const MAX_RESP: usize = 512;

pub struct PcscTransport {
    card: pcsc::Card,
}

impl PcscTransport {
    /// Connect to the first reader whose name contains `name_substr`
    /// (e.g. "Nucula", "Cherry", "ACR1252"), requiring a card present.
    pub fn connect_by_name(name_substr: &str) -> Result<Self, Error> {
        let ctx = pcsc::Context::establish(pcsc::Scope::User)
            .map_err(|_| Error::Transport("pcsc context"))?;
        let mut buf = [0; 2048];
        let readers = ctx
            .list_readers(&mut buf)
            .map_err(|_| Error::Transport("list readers"))?;
        let mut name = None;
        for r in readers {
            let s = r.to_string_lossy();
            if s.contains(name_substr) {
                name = Some(s.into_owned());
                break;
            }
        }
        let name = name.ok_or(Error::Transport("no matching reader"))?;
        let cstr = std::ffi::CString::new(name).map_err(|_| Error::Transport("reader name"))?;
        let card = ctx
            .connect(&cstr, pcsc::ShareMode::Shared, pcsc::Protocols::ANY)
            .map_err(|_| Error::Transport("connect card"))?;
        Ok(Self { card })
    }

    pub fn atr(&self) -> Result<Vec<u8>, Error> {
        let st = self
            .card
            .status2_owned()
            .map_err(|_| Error::Transport("card status"))?;
        Ok(st.atr().to_vec())
    }
}

impl CardTransport for PcscTransport {
    fn exchange(
        &mut self,
        cla: u8,
        ins: u8,
        p1: u8,
        p2: u8,
        data: &[u8],
    ) -> Result<(Vec<u8>, u16), Error> {
        let cmd = crate::encode_apdu(cla, ins, p1, p2, data);

        let mut resp = [0; MAX_RESP];
        let r = self
            .card
            .transmit(&cmd, &mut resp)
            .map_err(|_| Error::Transport("transmit"))?;
        let n = r.len();
        if n < 2 {
            return Err(Error::Transport("short response"));
        }
        let sw = u16::from_be_bytes([r[n - 2], r[n - 1]]);
        Ok((r[..n - 2].to_vec(), sw))
    }
}
