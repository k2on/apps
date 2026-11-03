//! Receipt photos (docs/plan-files.md): a [`Photo`] row is a job the
//! importer's peer claims; its file is read from the server's own store
//! (the device sent it beside the row), `receipt-ocr` reads it ([`paper`]),
//! and the receipt it says is written as any store's is
//! (`receipts.add_receipts`, store `paper`), paid by the transaction the
//! photo was taken for when it was. The photo is kept.
//!
//! A photo whose file has not arrived yet (made offline, its upload still
//! going) fails, and is tried again after the job's retry.

use std::path::{Path, PathBuf};

use ark_peer::Peer;
use money_domain::schema::{Photo, PhotoRead};
use money_domain::setup::{PaperLine, PaperReceipt};
use money_domain::types::{MatchReceipt, ReceiptImport, StoreItem, StoreReceipt};

use crate::paper;

/// The receipt a photo's reading says, as a store's adapter would give it:
/// keyed by the photo, on the day it was printed (else taken), its total
/// what was due.
pub fn receipt(photo: &Photo, r: &PaperReceipt) -> StoreReceipt {
    let item = |l: &PaperLine| StoreItem {
        name: l.name.clone(),
        sku: l.sku.clone(),
        quantity: l.qty.map_or(1000, |q| q as i64 * 1000),
        unit: "ea".into(),
        amount: -l.cents,
    };
    let due = r.due.or(r.target).unwrap_or(r.sum + r.tax);
    StoreReceipt {
        order_id: photo.id.hex(),
        date: r.date.clone().unwrap_or_else(|| money_domain::dates::day_of_ms(photo.taken_ms)),
        place: r.store.clone(),
        total: -due,
        items: r.items.iter().chain(&r.discounts).map(item).collect(),
    }
}

/// Reads one photo: what the importer's handler does with a job.
pub fn read(p: &mut Peer, program: &Path, data: &Path, photo: &Photo) -> Result<PhotoRead, String> {
    let bytes = p.file(&photo.file).ok_or("the photo has not reached the server yet")?;
    let readings = paper::ocr(program, data, &bytes).map_err(|e| format!("{e:#}"))?;
    let told = paper::read(&readings).told();
    let got = receipt(photo, &told);
    let receipt_id = format!("{}:{}", money_domain::stores::PAPER, got.order_id);
    let import = ReceiptImport {
        store: money_domain::stores::PAPER.into(),
        receipts: vec![got],
    };
    p.mutate("receipts.add_receipts", &import)?;
    if let Some(txn_id) = photo.txn_id {
        let pay = MatchReceipt {
            receipt_id: receipt_id.clone(),
            txn_id: Some(txn_id),
        };
        p.mutate("receipts.pay", &pay)?;
    }
    Ok(PhotoRead {
        receipt_id,
        state: told.state,
        changed: told.changed,
    })
}

/// The importer's peer reading photos: from the server's store, with
/// `program`.
pub fn handle(peer: &mut Peer, program: PathBuf, data: PathBuf) -> std::io::Result<()> {
    peer.keep_files(Box::new(ark_peer::Dir::of_server(&data)?));
    peer.handle::<Photo>(move |p, photo| read(p, &program, &data, photo));
    Ok(())
}
