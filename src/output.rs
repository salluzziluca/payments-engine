use std::collections::HashMap;

use crate::engine::Account;

pub fn write_accounts<W: std::io::Write>(
    writer: W,
    accounts: &HashMap<u16, Account>,
) -> Result<(), csv::Error> {
    let mut csv = csv::Writer::from_writer(writer);

    csv.write_record(["client", "available", "held", "total", "locked"])?;
    for (client_id, account) in accounts {
        csv.write_record([
            client_id.to_string(),
            format!("{:.4}", account.available()),
            format!("{:.4}", account.held()),
            format!("{:.4}", account.total()),
            account.is_locked().to_string(),
        ])?;
    }

    csv.flush()?;
    Ok(())
}
