// AccountAext parsing helpers
// Functions for parsing pre-execution AccountAext data

use revm_primitives::hex;
use revm_primitives::Address;
use std::collections::HashMap;
use tracing::{debug, warn};
use tron_backend_execution::AccountAext;

use super::address::strip_tron_address_prefix;
use crate::backend::AccountAextSnapshot;

/// Parse pre-execution AEXT snapshots from the gRPC request into a HashMap.
/// Converts Tron 21-byte addresses (0x41 prefix) to 20-byte EVM addresses for lookup.
pub(super) fn parse_pre_execution_aext(
    snapshots: &[AccountAextSnapshot],
) -> HashMap<Address, AccountAext> {
    let mut map = HashMap::new();

    for snapshot in snapshots {
        // Strip Tron 0x41 prefix to get 20-byte address
        match strip_tron_address_prefix(&snapshot.address) {
            Ok(addr_bytes) => {
                let address = Address::from_slice(addr_bytes);

                // Extract AEXT fields from protobuf
                if let Some(aext_proto) = &snapshot.aext {
                    debug!(
                        "Parsed pre-exec AEXT for address {}: net_usage={}, free_net_usage={}, energy_usage={}",
                        hex::encode(&snapshot.address),
                        aext_proto.net_usage,
                        aext_proto.free_net_usage,
                        aext_proto.energy_usage
                    );

                    let aext = AccountAext {
                        net_usage: aext_proto.net_usage,
                        free_net_usage: aext_proto.free_net_usage,
                        energy_usage: aext_proto.energy_usage,
                        latest_consume_time: aext_proto.latest_consume_time,
                        latest_consume_free_time: aext_proto.latest_consume_free_time,
                        latest_consume_time_for_energy: aext_proto.latest_consume_time_for_energy,
                        net_window_size: aext_proto.net_window_size,
                        net_window_optimized: aext_proto.net_window_optimized,
                        energy_window_size: aext_proto.energy_window_size,
                        energy_window_optimized: aext_proto.energy_window_optimized,
                    };

                    map.insert(address, aext);
                }
            }
            Err(e) => {
                warn!("Failed to parse address from pre-exec AEXT snapshot: {}", e);
            }
        }
    }

    map
}
