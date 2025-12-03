# YPF Ruta
Statement available at [this link](https://concurrentes-fiuba.github.io/2025_2C_tp2.html) or its local copy at [docs/enunciado_tp2](docs/enunciado_tp2.pdf).

## Members
- **Team name**: `:(){ :|:& };:`
- **Reviewer**: Darius Maitia

| Name                | Student ID |
|---------------------|------------|
| [Alejo Ordonez](https://github.com/alejoordonez02) | 108397 |
| [Francisco Pereyra](https://github.com/fapereyra) | 105666 |
| [Lorenzo Minervino](https://github.com/lminervino18) | 107863 |
| [Alejandro Paff](https://github.com/AlePaff) | 103376 |


## Start project

Run the following command in the terminal to automatically start the server side (leader + replicas + simulated stations):

```bash
./scripts/launch.sh <N_replicas> <M_stations>
```

> `<N_replicas>` = number of cluster replicas  
> `<M_stations>` = number of node_client (stations) to start  

---

### Distributed server (`server`)

If you want to launch the nodes manually (without script), use the `server` binary with `clap`:

```bash
# Leader
cargo run --bin server -- \
  --address 127.0.0.1:5000 \
  --coords -31.4 -64.2 \
  --pumps 4 \
  leader --max-conns 32

# Replica (pointing to the leader)
cargo run --bin server -- \
  --address 127.0.0.1:5001 \
  --coords -34.6 -58.4 \
  --pumps 4 \
  replica --leader-addr 127.0.0.1:5000 --max-conns 32
```

Main parameters:

- `--address <IP:PORT>`: address where the node listens.  
- `--coords <LAT> <LON>`: geographic coordinates of the node (for the map).  
- `--pumps <N>`: number of pumps simulated by the internal station.  
- Subcommands:
  - `leader --max-conns <N>`: starts a leader node.
  - `replica --leader-addr <IP:PORT> --max-conns <N>`: starts a replica connected to the leader.
  - `station { ... }`: dedicated station mode (not yet implemented, `todo!`).

> Both the **leader** and each **replica** internally start a station simulator (`Station::start(pumps)`) that reads **stdin** and allows you to trigger loads from the terminal (see section *Station simulator by stdin*).

---

### Station / client node (`node_client`)

The `node_client` is a thin client that simulates the pumps of a station and forwards operations to the cluster:

```bash
# General usage
node_client <bind_addr> <max_connections> <num_pumps> <known_node_1> [<known_node_2> ...]

# Example
cargo run --bin node_client -- \
  127.0.0.1:6000 \
  128 \
  5 \
  127.0.0.1:5000 127.0.0.1:5001
```

Where:

- `<bind_addr>`: IP:PORT where the `node_client` listens.
- `<max_connections>`: maximum accepted connections.
- `<num_pumps>`: number of pumps simulated.
- `<known_node_*>`: one or more cluster nodes (leader and/or replicas) to which it sends operations.

> Like the server, the `node_client` internally creates a `Station::start(num_pumps)` that uses **stdin** to simulate pump loads.

---

### Station simulator by stdin (server and node_client)

Both the `server` binary (leader / replica) and the `node_client` binary share the same station simulator.  
Each process that starts a `Station` shows a prompt like:

```text
[Station] Starting station simulator with N pumps (ids: 0..=N-1)
=== Station / pump simulator commands ===
...
```

From **stdin** you can send commands with the following format:

```text
<pump_id> <account_id> <card_id> <amount>
```

Examples:

```text
0 100 10 50.0     # pump 0, account 100, card 10, amount 50.0
1 200 20 125.5    # pump 1, account 200, card 20, amount 125.5
```

- `pump_id`: pump index (0..=N-1).  
- `account_id`: account id to debit.  
- `card_id`: card id within that account.  
- `amount`: amount to consume.

When a valid line is sent:

- The simulator generates a unique `request_id`.
- Marks the `pump_id` as **busy** until it receives the response.
- Sends a `StationToNodeMsg::ChargeRequest` to the node (server or node_client).

Node responses arrive as `NodeToStationMsg::ChargeResult` and are displayed as:

```text
[Station][RESULT] pump=0 -> CHARGE OK (request_id=123, account=100, card=10, amount=50)
[Station][RESULT] pump=1 -> CHARGE DENIED (request_id=456, account=200, card=20, amount=125.5, error=...)
```

---

#### Special simulator commands

In addition to load lines, the simulator understands the following plain text commands:

```text
help
quit
exit
disconnect
connect
```

- `help`  
  Shows simulator help (command format and examples).

- `quit` / `exit`  
  Stops the station simulator of that process:
  - Prints: `[Station] Received 'quit', shutting down simulator.`
  - The main process continues running, but no more stdin commands are read for that station.

- `disconnect`  
  Sends `StationToNodeMsg::DisconnectNode` to the node:
  - Asks the node to switch to **OFFLINE** mode (according to the node's internal logic).
  - Useful for testing operation queues and reconnection.

- `connect`  
  Sends `StationToNodeMsg::ConnectNode` to the node:
  - Asks the node to return to **ONLINE** mode.
  - Useful for testing how the pending operation queue is emptied.

---

### Interactive account administrator (`administrator`)

The administrator connects to a cluster node to query and modify account/card limits.  
It is **interactive via stdin** (a small REPL with text commands).

```bash
# General usage
administrator <bind_addr> <target_node_addr> <account_id>

# Examples
cargo run --bin administrator -- \
  127.0.0.1:9000 \
  127.0.0.1:5000 \
  100

cargo run --bin administrator -- \
  127.0.0.1:9001 \
  127.0.0.1:5001 \
  200
```

- `<bind_addr>`: local address from which the administrator connects.
- `<target_node_addr>`: cluster node to which requests are sent (typically the leader).
- `<account_id>`: fixed account on which this administrator instance will operate.

Once started, the prompt appears:

```text
Interactive administrator is ready.
Type 'help' to see commands, 'exit' to quit.
admin>
```

Available commands within the administrator:

- `help`  
  Shows the list of commands.

- `exit` / `quit`  
  Closes the administrator.

- `limit-account <amount>`  
  Updates the limit of the fixed account (`account_id` with which the process was started).  
  - `amount > 0` → sets that limit.  
  - `amount <= 0` → removes the limit (sets it to `None`).

- `limit-card <card_id> <amount>`  
  Updates the limit of a specific card of the account.

- `account-query`  
  Queries the account status (balances, consumption per card, etc.).

- `bill`  
- `bill 2025-10`  
  Triggers account billing, optionally filtering by period `YYYY-MM`.  

Session examples:

```text
admin> limit-account 1000.0
admin> limit-card 10 200.0
admin> account-query
admin> bill
admin> bill 2025-10
admin> exit
```




## Report
The report presented below is available in PDF $\LaTeX{}$ format at [docs/informe.pdf](docs/informe/informe.pdf).

# Introduction

This work presents **YPF Ruta**, a distributed system that allows companies
to centralize payment and control of fuel expenses for their vehicle fleet.

Each company has a main account and cards associated with different drivers.
When a vehicle needs to refuel at any of the more than 1600 stations
distributed throughout the country, the driver uses their card to authorize the operation. The system
records each consumption and, later, bills the company the total amount accumulated on all
its cards during the billing period.

# General overview of the system

This section broadly describes how **YPF Ruta** is organized: which actors
are involved, how they connect to each other, and what problems the system seeks to solve at the
business and distributed infrastructure level.

## Usage context

YPF Ruta models a scenario in which companies with vehicle fleets refuel at a
wide network of service stations. Each driver uses a card associated with their company's account to authorize the load, while the central system:

- validates the consumption limits of the card and the account,
- records each charging operation,
- allows consumption queries and subsequent billing.

From a technical point of view, the system assumes a distributed environment with multiple nodes
communicated over the network, possible node failures, temporary disconnections, and stations that can
temporarily operate without direct connectivity to the consensus cluster.

## System objectives and scope

The main objective of YPF Ruta is to provide a distributed infrastructure that allows:

- Centralize fuel expense control for a fleet through accounts and cards.
- Replicate the state of accounts and cards among several nodes (leader and replicas) maintaining
  consistency through a replicated log.
- Tolerate leader failure through automatic election of a new leader (Bully algorithm).
- Support charging policies in disconnection scenarios (OFFLINE mode) and reconcile
  consumption once connectivity is restored.
- Offer an administration interface (CLI) to limit accounts and cards, query
  consumption, and trigger billing processes.

The system is implemented as an academic prototype: storage is in memory, business logic is modeled through actors, and communication between nodes is done over TCP with a custom application protocol.


# Distributed server

This section describes the internal architecture of the **YPF Ruta** server as a distributed system. The focus is on the consensus cluster (leader and replicas), the flow of business operations, and how consistent state is maintained between nodes through a replicated log.

## Consensus cluster (leader and replicas)

The server is organized around a *consensus cluster* formed by a **leader** node and
one or more **replica** nodes. All of them maintain a logical copy of the same business state:
accounts, cards, limits, and consumption. That in-memory state is implemented through an
actor model (ActorRouter, AccountActor, CardActor), isolating business logic from
distributed communication logic.

From the cluster's point of view:

- The **leader** is the only node that decides the *global order* of operations and coordinates the
  commit. Each time it receives a new operation (for example, a `Charge` generated by a
  station or a `LimitAccount` requested by an administrator) it records it in its local log and
  triggers the replication process to the replicas.
- The **replicas** receive log entries from the leader and apply them in the same order,
  keeping their own actor system synchronized with the leader's. They do not make ordering or
  commit decisions themselves: they follow the sequence sent by the leader.

The simplified flow for an operation is as follows (scheme inspired by Raft):

1. A high-level operation (`Operation`) arrives at some node of the system (leader, replica or
   station that proxies) and ends up being delivered to the leader as a `Request` message.
2. The leader assigns a new operation identifier (`op_id`), saves it in its `PendingOperation`
   structure, and creates an associated log entry.
3. The leader sends each replica a `Log { op_id, op }` message through the network layer
   (abstraction `Connection`).
4. Each replica, upon receiving a `Log`, stores the operation in its own log and executes, through
   its `Database` (ActorRouter + actors), the immediately previous operation. Once execution
   finishes, the replica responds to the leader with an `Ack { op_id }` message.
5. The leader counts the `Ack`s received. When an operation has *ack* from the
   majority of replicas, it is considered *committed*. At that moment the leader executes the
   operation in its own actor system and translates the result into a response:
   - if the origin was a station, it generates a `NodeToStationMsg::ChargeResult` to the Station;
   - if the origin was a TCP client (administrative CLI), it sends a `Message::Response` with the
     corresponding `OperationResult`.

In this way, the replicated log defines a total order of operations and ensures that both the
leader and the replicas apply exactly the same sequence to their in-memory state. Cluster
consistency is achieved without sharing memory between nodes: all coordination is done
through messages (`Request`, `Log`, `Ack`, `Response`) over TCP.

## Stations and administrative clients

Although the consensus cluster is formed only by the leader and replicas, **not all
service stations are part of that cluster**. In YPF Ruta we distinguish two types of
server clients:

- Stations that do not play a role in the consensus cluster (nodes “external” to consensus).
- Administrative clients (CLI) used by companies to manage their accounts.

Both types of client generate high-level operations (`Operation`) that, directly or
indirectly, end up reaching the leader.

### Service stations

Each station has a pump simulator (`Station`) that:

- reads commands from stdin (one per charging operation),
- translates them into high-level messages (`StationToNodeMsg::ChargeRequest`),
- and waits for the corresponding response (`NodeToStationMsg::ChargeResult`).

Depending on where the station is running:

- If it runs in the same process as a **leader node** or a **replica**, the `Station`
  connects internally to that node and it acts as the station's “front” to the consensus cluster.
- If the station is completely **outside the cluster**, it connects via TCP to some node
  (typically the leader) and that node takes the role of entry point for its operations.

In any case, the station does not need to know the internal topology of the cluster or how
replication is implemented: it only sends charging requests and receives admitted/denied results
with optional error information (`VerifyError`).

### Administrative clients (CLI)

The administrative client is implemented as an independent binary (`client`) that offers a
CLI to interact with the system. Through this client it is possible to:

- limit the available amount in an account (`Operation::LimitAccount`),
- limit the available amount on a card (`Operation::LimitCard`),
- query account consumption (`Operation::AccountQuery`),
- start billing processes (`Operation::Bill`).

The CLI serializes these operations into its own binary format and sends them via TCP to the server.
From the cluster's perspective, a CLI command is simply another `Operation` that enters
the leader via a `Request` message, follows the same replication and commit flow as a charge
originated at a station, and ends in an `OperationResult` response that the client displays via
stdout.

## Fault tolerance and leader election (Bully)

The consensus cluster is designed to continue operating even if some nodes stop
responding, particularly the leader. As long as at least one replica and the majority of nodes remain
active, the system can elect a new leader and continue processing operations.

When leader failure is detected, one of the replicas starts a **leader election** using the
Bully algorithm. The basic idea is:

- Each cluster node has a unique identifier (numeric ID).
- The node that detects the failure nominates itself as a candidate and sends *election* messages to nodes
  with a higher ID than its own.
- If no node with a higher ID responds, the candidate proclaims itself the new leader and announces its role to
  the rest of the cluster.
- If a node with a higher ID responds, that node “takes over” and continues the election process,
  until finally the highest available ID node becomes leader.

Thus, leadership always falls to the “strongest” (highest ID) node that remains active.
For stations and administrative clients, this process is almost transparent: they can
continue sending operations to cluster nodes, which internally redirect them
to the current leader.

# Clients

This section describes the two main types of clients that interact with
**YPF Ruta**: service stations (through their pumps) and
administrative users of companies, who operate via a command line client (CLI).

## Service stations and pumps

Each station is modeled as a terminal that groups several pumps. In the prototype,
this terminal is represented by the `Station` component, which:

- reads commands entered by the operator (one for each load attempt),
- translates them into high-level charging requests to the node it is connected to,
- and displays the result of the operation (approved or rejected, with the reason) on screen.

From the station's perspective, the typical flow is:

1. The operator enters the operation data (pump, account, card, amount).
2. The station sends a charging request to the system.
3. The system responds indicating whether the operation was accepted or not, and why
   (for example, card or account limit exceeded).

The details of how that operation is replicated within the cluster (leader, replicas, log, ACKs)
are hidden behind this simple “charging request / result” interface.

## Administrative client (CLI) for companies

Companies using YPF Ruta also have a text-mode administrative client (CLI), designed for management tasks on their accounts. Through this client it is possible to:

- set or modify the global limit of an account,
- set individual limits for each associated card,
- query the accumulated consumption of an account and its breakdown by card,
- start billing processes on an account.

Each CLI command is internally translated into an `Operation` (for example,
`LimitAccount`, `LimitCard`, `AccountQuery` or `Bill`) that is sent to the server and follows the
same consensus flow as charging operations. The administrative user, however, only sees a simple interface for querying and updating their account parameters.


# Node implementation

Each server process (leader or replica) is modeled as a **node** that communicates
simultaneously with three different “worlds”:

- other cluster nodes (consensus and replication),
- the local station (simulated pumps),
- the logical database implemented with actors.

These behaviors are abstracted in the `Node` trait, which is implemented by `Leader` and
`Replica`.

## Overview and main node loop

The heart of the implementation is the main loop defined in `Node::run`. This asynchronous loop runs while the node is alive and resolves, using a `tokio::select!`,
three types of events:

- messages arriving from other nodes via the network layer (`Connection`),
- messages arriving from the local station (`StationToNodeMsg`),
- events emitted by the actor world (`ActorEvent`) via `Database`.

Depending on the type of event, the node delegates to the trait methods:

- `handle_node_msg` for consensus and replication messages (`Request`, `Log`, `Ack`,
  `Election`, etc.),
- `handle_station_msg` for charging requests and ONLINE/OFFLINE mode changes,
- `handle_actor_event` for business operation results.

Thus, the specific logic of leader or replica is concentrated in the trait implementation, while the main loop is common to all roles.

## Communication with other nodes (Connection / Message)

Communication between nodes is encapsulated in the `Connection` abstraction, which offers a
uniform interface for:

- accepting incoming connections and establishing outgoing connections,
- sending typed messages (`Message`) to an address (`SocketAddr`),
- receiving messages from the network asynchronously.

The `Message` enum represents all “node world” messages:

- operation messages (`Request`, `Response`, `Log`, `Ack`),
- leader election messages (`Election`, `ElectionOk`, `Coordinator`),
- cluster membership messages (`Join`, `ClusterView`, `ClusterUpdate`).

The `handle_node_msg` method of the `Node` trait acts as a *dispatcher*: it unpacks the
received `Message` and calls the specific handlers (`handle_request`, `handle_log`,
`handle_ack`, `handle_election`, etc.) that implement leader or replica semantics.

## Communication with the station (Station, StationToNodeMsg, NodeToStationMsg)

Interaction with a station's pumps is modeled by the `Station` type, which
runs in a background task and communicates with the node via asynchronous channels:

- `StationToNodeMsg`: messages the station sends to the node.
- `NodeToStationMsg`: messages the node sends back to the station.

The main messages are:

- `StationToNodeMsg::ChargeRequest` to request a charge (account, card, amount).
- `StationToNodeMsg::DisconnectNode` and `ConnectNode` to change ONLINE/OFFLINE mode.
- `NodeToStationMsg::ChargeResult` to return the final result of a charging operation,
  including whether it was allowed or not and a possible `VerifyError`.
- `NodeToStationMsg::Debug` to send informational messages to the operator.

The `handle_station_msg` method of the `Node` trait encapsulates the policy of what to do for each
message: in the case of a `ChargeRequest`, it builds an `Operation::Charge` and injects it into
the normal cluster flow (passing through the leader and replication).

## Communication with the logical database (Database, ActorEvent)

The system's **logical database** is implemented as a set of actors and is
encapsulated behind the `Database` type. From the node's point of view, `Database` offers
two operations:

- `send(DatabaseCmd)`: to send high-level commands (for now, `Execute { op_id,
  operation }`).
- `recv()`: to receive events produced by the actor world (`ActorEvent`).

The main actor is the `ActorRouter`, which coordinates:

- account actors (`AccountActor`) responsible for limits and consumption at the account level,
- card actors (`CardActor`) responsible for limits and consumption per card.

When the leader decides to *commit* an operation, instead of applying business logic
directly, it sends a `DatabaseCmd::Execute` to `Database`. The result then arrives as an
`ActorEvent::OperationResult`, which the node translates into:

- a response to the station (`NodeToStationMsg::ChargeResult`), or
- a response to the administrative client (`Message::Response` with an `OperationResult`).

This separation allows the node to focus on distributed coordination and fault tolerance, while consistency and verification of business operations are resolved within the actor model.


# Actor model of the database

The logical database of **YPF Ruta** is modeled with the **actor** pattern: each business entity (account, card) is an actor with its own state, which is only modified through messages. The node never accesses that state directly: it sends high-level operations and receives a typed result.

## ActorRouter

`ActorRouter` is the entry point to the actor world. From the node's point of view:

- receives a business operation (for example, `Execute(op_id, Operation)`),
- ensures the necessary actors are created (accounts and cards),
- coordinates message exchange between them,
- and, when everything is done, emits a single result
  `OperationResult(op_id, Operation, Result)`.

In summary, it takes a high-level request, breaks it down into internal messages, and reconcentrates all those interactions into a single response for the node.

## AccountActor

Each `AccountActor` encapsulates the state of an account:

- global account limit,
- accumulated consumption,
- and, when needed, temporary state for queries with card detail.

Conceptually it handles three types of messages:

- `ApplyCharge(amount, from_offline_station)`
- `ApplyAccountLimit(new_limit)`
- `AccountQueryStep(card_id, consumed)`

With them:

- validates and applies charges at the account level (respecting the global limit),
- validates and updates the global limit,
- and adds the information reported by the cards to build the response
  to an account query.

## CardActor

Each `CardActor` represents an individual card:

- card limit,
- accumulated consumption,
- and a task queue to process its operations in order.

It conceptually receives messages like:

- `ExecuteCharge(amount, from_offline_station)`
- `ExecuteLimitChange(new_limit)`
- `ReportStateForAccountQuery()`

With these messages:

- verifies the card limit before a charge,
- delegates to the account the verification of the global limit,
- and responds with its current consumption when building an account query.

## Messages, operations and results (Operation, OperationResult)

To decouple the node from the actor model, a limited set of high-level operations
and results is defined:

`Operation` =
- `Charge(account_id, card_id, amount, from_offline_station)`
- `LimitAccount(account_id, new_limit)`
- `LimitCard(account_id, card_id, new_limit)`
- `AccountQuery(account_id)`
- `Bill(account_id, period)`

`OperationResult` =
- `ChargeResult(Ok | Failed(VerifyError))`
- `LimitAccountResult(Ok | Failed(VerifyError))`
- `LimitCardResult(Ok | Failed(VerifyError))`
- `AccountQueryResult(account_id, total_spent, per_card_spent)`

The complete flow is:

1. The node receives an `Operation` and decides when to *commit* according to consensus.
2. Once decided, it sends `Execute(op_id, Operation)` to the actor model.
3. Account and card actors process the operation only through messages.
4. `ActorRouter` returns an `OperationResult`, which the node translates into the response
   to the station or administrative client.


# Communication protocol

## Application protocol (message format and serialization)

The different **YPF Ruta** processes communicate through their own binary application protocol. Each message begins with a type byte that identifies the variant
(`Request`, `Log`, `Ack`, `Election`, etc.), followed by the specific fields of that type
encoded in binary (integers, `f32`, flags, etc.).

This format is used uniformly both between consensus cluster nodes and between
clients (stations or administrative CLI) and the node they connect to. Above these
low-level messages, the system logic works with business operations (`Operation`)
and results (`OperationResult`), which allows separating domain decisions from
serialization details.

## Transport protocol (TCP)

**TCP** is used for transport, providing a reliable, connection-oriented channel with
ordered delivery of bytes. These properties are fundamental in a charging system,
where each message represents a financial operation and each log entry must be applied in
the same order on all cluster nodes. In addition, TCP connection closure or error
is used as a signal of node or client failure, triggering fault tolerance mechanisms when appropriate.


# Charging policy at stations without connection

## Node outside the cluster

When a station that **is not part of the consensus cluster** loses connection to the network, the central system cannot verify limits or balances. In that scenario, the decision to accept or reject the charge is entirely on the station side: it can operate in “trust and record locally” mode or “do not authorize without connection” mode, but any policy it adopts will necessarily be local and not consistent with the rest of the system until connectivity is restored.

## Replica / leader node in OFFLINE mode

If the node that loses connectivity is a cluster node (leader or replica), it can explicitly enter **OFFLINE** mode. In this mode, the node stops participating in consensus but continues serving its pumps: each charge is accepted immediately and the operation is recorded in a queue of “offline charges” marked as such (`from_offline_station = true`). The idea is to prioritize service continuity at the station, postponing global limit verification until the node is back online.

## Reconciliation upon connection recovery

When returning to **ONLINE** mode, the node sequentially replays all queued operations against the logical database and replicates them to the consensus cluster. Since those operations are tagged as coming from an offline station, they are applied directly to the state (without blocking the client again) to rebuild a consistent history. If a new leader was elected during the disconnection, pending updates are sent to that current leader, so the cluster converges again to a single agreed state.


# Changes from the first delivery

During the system implementation, significant changes were made compared to the initial design presented in the first delivery. These changes arose from a better understanding of the system requirements and the available distributed concurrency tools.

## 1 - Architecture change: from per-card clusters to centralized consensus
The fundamental change was moving from a system with **multiple decentralized clusters per card** to a **centralized consensus cluster with replicated log**. This decision greatly simplified the implementation, allowed for a clearer demonstration of distributed concurrency tools (leader election, consensus), and resulted in a more robust and easier to reason about system.

**Initial design:** The system was planned with three types of clusters:
Pump cluster at each station, subscriber node cluster per card (with card leader) and account cluster that coordinated card leaders.
This architecture sought to optimize communication through geographic locality, where each card had its own cluster of subscriber nodes that replicated information, with a TTL (Time-To-Live) mechanism to unsubscribe from unused cards.

**Final implementation:** A **centralized consensus** model was adopted with:
- A single **leader** node that coordinates all operations
- **Replica** nodes that maintain synchronized copies of the state
- A **replicated log** inspired by Raft to guarantee consistency

**Reasons for the change:**
1. **Simplicity:** The centralized consensus model is simpler to implement correctly and reason about its behavior.
2. **Strong consistency:** The replicated log ensures that all operations are applied in the same order on all nodes, avoiding complex inconsistencies. As well as consensus and fault tolerance techniques.
3. **Prototype scope:** For an academic system with 1600 simulated stations, optimization by geographic locality added unnecessary complexity.

## 2 - Actor model: from distributed actors to local actors
**Initial design:** Three types of distributed actors were proposed: **Subscriber** Actor, **Card Leader** Actor, and **Account** Actor.

Actors communicated between nodes via viral message propagation.

**Final implementation:** Actors remained as **local** entities within each node and also allow
- `ActorRouter`: entry point and coordinator
- `AccountActor`: manages account state (limit and consumption)
- `CardActor`: manages card state (limit and consumption)

**Reasons for the change:**
1. **Separation of responsibilities:** Actors focus exclusively on business logic (validate limits, accumulate consumption), while distributed consensus is handled by the cluster of nodes.
2. **Model simplicity:** Actors only communicate via messages within the same process, eliminating the complexity of inter-node communication at the actor level.

## 3 - Elimination of TTL mechanism and dynamic subscriptions

**Initial design:** Nodes dynamically subscribed to cards according to geographic use, with a TTL mechanism for automatic unsubscription.

**Final implementation:** This mechanism was completely eliminated. All cluster nodes replicate all state from the start.

## 4 - Consensus protocol: replicated log with majority of ACKs

**Initial design:** No formal consensus protocol between cluster nodes was specified.

**Final implementation:** A simplified replicated log protocol was implemented:
1. The leader assigns a sequential `op_id` to each operation
2. The leader sends `Log { op_id, operation }` to all replicas
3. Each replica executes the **previous** operation and responds with `Ack { op_id }`
4. The leader waits for ACKs from the majority before considering the operation *committed*
5. The leader executes the operation in its own actor system and responds to the client

## 5 - Handling disconnections: OFFLINE mode with reconciliation

**Initial design:** It was mentioned that stations without connection could perform charges that were queued for later synchronization.

**Final implementation:** The concept of **OFFLINE mode** was formalized:
- Any node (leader or replica) can explicitly enter OFFLINE mode
- In OFFLINE mode, charges are accepted immediately and queued with flag `from_offline_station = true`
- Upon returning to ONLINE mode, all queued operations are replayed against the cluster
- Offline operations are applied without re-validating limits (they were already consumed)

## 6 - Binary serialization protocol

**Initial design:** A protocol was specified with 1 byte for message type and specific fields per type.

**Final implementation:** The general idea was maintained but refined:
- Initial byte identifies the type of `Message` or `Operation`
- Fields serialized in fixed order (u32, u64, f32, etc.)
- No compression or advanced optimizations (simplicity over efficiency)


# Conclusions

The development of **YPF Ruta** allowed the design and implementation of a distributed system capable of centralizing fuel expense control for a fleet, while maintaining a high level of availability at stations. The combination of a consensus cluster (leader + replicas) with an actor model for the logical database clearly separates concerns: on one hand, replication and agreement on the order of operations; on the other, consistency of business rules on accounts and cards.

The role of the **leader** as the point of commitment for operations, together with the **replicas** that maintain synchronized copies of the state, provides node-level fault tolerance. Leader election based on the **Bully** algorithm allows recovery of a valid coordinator when the current node fails, preventing the system from indefinitely lacking someone responsible for advancing the operation log. From the clients' perspective (stations and administrative CLI) this transition is transparent: just resend the operation to the node that effectively responds.

The use of the **actor model** to represent accounts and cards introduces a concurrency scheme that avoids sharing mutable memory between threads and nodes. All state modifications are expressed as high-level messages (`Operation`) and their corresponding results (`OperationResult`), which simplifies both reasoning about business logic and integration with the replication protocol. The explicit policy for **OFFLINE mode** at stations or cluster nodes, along with subsequent reconciliation, allows balancing service continuity and eventual system consistency.

Overall, the proposed architecture shows that it is possible to build a distributed charging service that combines: replication and consensus on operations, encapsulation of business logic in actors, and a simple binary communication protocol sufficient for the domain's needs. From this base, natural extensions can be explored, such as new administrative operations, usage metrics per station, or more sophisticated risk management policies in m


