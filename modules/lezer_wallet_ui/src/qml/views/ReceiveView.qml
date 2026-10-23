import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Format.js" as Fmt

// Public accounts receive at their id; private accounts at a lezer:priv1… receive address
// (nullifier + ML-KEM viewing key). Incoming private funds appear after "Sync private".
Item {
    id: view
    property var store
    property string address: ""

    readonly property var account: store.accounts[box.currentIndex] || null

    function load() {
        address = ""
        if (!account) return
        if (account.kind === "public") { address = account.account_id; return }
        store.call({ op: "receive_address", account: account.account_id }, function (k) { view.address = k.receive_address })
    }

    ColumnLayout {
        width: Math.min(parent.width, 680)
        spacing: 10
        ComboBox {
            id: box
            Layout.fillWidth: true
            model: store.accounts.map(function (a) { return (a.kind === "private" ? "🔒 " : "") + Fmt.label(a) })
            onCurrentIndexChanged: view.load()
            Component.onCompleted: view.load()
        }
        Label {
            Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#9aa0a6"
            text: view.account && view.account.kind === "private"
                  ? "Share this private receive address. Senders cannot see your balance or your other activity."
                  : "Share this public address. Everything sent to it is visible on chain."
        }
        TextArea { id: addr; Layout.fillWidth: true; Layout.preferredHeight: 140; readOnly: true; wrapMode: Text.WrapAnywhere; text: view.address; font.family: "Menlo"; color: "#e8eaed" }
        Button { text: "Copy"; enabled: view.address.length > 0; onClicked: { addr.selectAll(); addr.copy(); addr.deselect() } }
    }
}
