import QtQuick

// Drop-in QML client for Basecamp ui_qml dApps using the Lezer wallet.
// Your module is identified to the wallet by its host-authenticated name; the session is
// bound to it. connect()/propose() resolve after the user decides in the wallet UI.
//
//   LezerProvider { id: wallet; displayName: "My dApp"
//       onConnected: (accounts) => console.log(accounts)
//       onProposalResolved: (receipt) => console.log(receipt.tx_hash) }
QtObject {
    id: provider

    readonly property string moduleName: "lezer_wallet_provider"
    property string displayName: "dApp"
    property bool requestPrivateBalanceRead: false
    property string sessionToken: ""
    property var accounts: []
    property bool busy: false

    signal connected(var accounts)
    signal proposalResolved(var receipt)
    signal failed(string code, string message)

    function _decode(payload) {
        var v
        try { v = JSON.parse(payload) } catch (e) { return { error: { code: "BRIDGE", message: String(payload) } } }
        if (typeof v === "string") { try { v = JSON.parse(v) } catch (e2) { return { error: { code: "BRIDGE", message: v } } } }
        if (v && typeof v.error === "string") v.error = { code: "BRIDGE", message: v.error }
        return v
    }

    function _call(method, args, onOk) {
        if (typeof logos === "undefined" || !logos) { provider.failed("BRIDGE", "Logos bridge unavailable"); return }
        logos.callModuleAsync(moduleName, method, args, function (payload) {
            var r = provider._decode(payload)
            if (r && r.error) { provider.busy = false; provider.failed(r.error.code || "", r.error.message || "") }
            else if (onOk) onOk(r)
        })
    }

    property var _pending: ({})   // request_id -> "connect" | "proposal"
    property Timer _poll: Timer {
        interval: 1000; repeat: true; running: false
        onTriggered: {
            var ids = Object.keys(provider._pending)
            if (ids.length === 0) { stop(); return }
            ids.forEach(function (id) {
                provider._call("get_request_status", [id], function (r) {
                    if (r.status === "pending" || r.status === "executing") return
                    var kind = provider._pending[id]
                    var p = Object.assign({}, provider._pending); delete p[id]; provider._pending = p
                    provider.busy = false
                    if (r.status === "rejected") provider.failed(r.code, r.reason)
                    else if (kind === "connect") {
                        provider.sessionToken = r.session_token
                        provider.accounts = r.authorized_accounts
                        provider.connected(r.authorized_accounts)
                    } else provider.proposalResolved(r.result)
                })
            })
        }
    }

    function _track(id, kind) {
        var p = Object.assign({}, provider._pending); p[id] = kind; provider._pending = p
        provider._poll.start()
    }

    function connect() {
        busy = true
        _call("connect", [JSON.stringify({ displayName: displayName, requestPrivateBalanceRead: requestPrivateBalanceRead })],
              function (r) { provider._track(r.request_id, "connect") })
    }

    function propose(intent) {
        if (!sessionToken) { failed("LZR-2001", "not connected"); return }
        busy = true
        _call("propose_transaction", [sessionToken, JSON.stringify(intent)],
              function (r) { provider._track(r.request_id, "proposal") })
    }

    function getBalance(accountId, onOk) {
        _call("get_balance", [sessionToken, accountId], onOk)
    }

    function disconnect() {
        if (!sessionToken) return
        _call("disconnect", [sessionToken], function () { provider.sessionToken = ""; provider.accounts = [] })
    }
}
