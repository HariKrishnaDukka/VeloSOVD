# SOVD virtual network

The network lifecycle is managed by `tools/setup_sovd_network.sh`.

## Topology

| Link | Central endpoint | ZCU endpoint | Subnet |
|---|---|---|---|
| Central–Front | `10.77.10.1/30` | `10.77.10.2/30` | `10.77.10.0/30` |
| Central–Rear | `10.77.10.5/30` | `10.77.10.6/30` | `10.77.10.4/30` |

Namespaces:
- `sdv-central-ns`
- `sdv-front-ns`
- `sdv-rear-ns`

CAN interfaces:
- `vcan-front` inside `sdv-front-ns`
- `vcan-rear` inside `sdv-rear-ns`

## Commands

```bash
sudo tools/setup_sovd_network.sh up
sudo tools/setup_sovd_network.sh status
sudo tools/setup_sovd_network.sh down
```

`up` refuses to adopt pre-existing namespace or host-link names. It creates an ownership marker only after successful setup; failed setup rolls back only resources created during that invocation. `down` requires that marker and deletes only the three SOVD namespaces.

The script does not modify the unrelated `central-ns`, `zcu-ns`, or `vcan0` resources.

## Important limitations

- This is a Linux virtual-network test harness, not a production vehicle network configuration.
- `vcan` does not model CAN electrical characteristics, arbitration timing, bus errors, termination, or physical-layer faults.
- The two `/30` Ethernet links are directly connected point-to-point links. No default route or forwarding is configured.
- The script currently uses the topology values duplicated in the script. Keep these values synchronized with `platform/config/vehicle-topology.toml`; a future improvement should parse and validate the TOML rather than maintain two sources of truth.
- Do not run `down` while project processes are using these namespaces.
