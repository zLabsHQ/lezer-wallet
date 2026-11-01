import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Format.js" as Fmt

// Accounts with balances. Public balances are read live from the sequencer; private
// balances come from locally decrypted state (Sync picks up incoming notes).
Item {
    id: view
    property var store
    property var balances: ({})
    property string syncing: ""

    function refreshBalances() {
        store.accounts.forEach(function (a) {
            store.call({ op: "balance", account: a.account_id }, function (b) {
                var m = Object.assign({}, view.balances); m[a.account_id] = b; view.balances = m
            }, function () {})
        })
    }
    // Reload when the account set changes (e.g. after unlock, create, or sync discovery).
    property string accountKey: store.accounts.map(function (a) { return a.account_id }).join(",")
    onAccountKeyChanged: refreshBalances()
    Component.onCompleted: refreshBalances()
    Connections { target: view.store; function onJobFinished() { view.refreshBalances() } }

    ColumnLayout {
        anchors.fill: parent
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            Button { text: "New public account"; onClicked: store.call({ op: "create_account", kind: "public", label: null }, function () { store.refresh(); view.refreshBalances() }) }
            Button { text: "New private account"; onClicked: store.call({ op: "create_account", kind: "private", label: null }, function () { store.refresh(); view.refreshBalances() }) }
            Item { Layout.fillWidth: true }
            Button { text: "Refresh"; onClicked: view.refreshBalances() }
            Button {
                text: view.syncing.length ? "Syncing…" : "Sync private"
                enabled: view.syncing.length === 0
                onClicked: {
                    view.syncing = "1"
                    store.runJob({ op: "sync" }, "sync", function () { view.syncing = ""; view.refreshBalances() })
                }
            }
        }

        ListView {
            Layout.fillWidth: true; Layout.fillHeight: true
            clip: true; spacing: 6
            model: store.accounts
            delegate: Rectangle {
                width: ListView.view.width; height: 64; radius: 8
                color: store.snapshot && store.snapshot.active_account_id === modelData.account_id ? "#1d2a3b" : "#1b1f27"
                readonly property var bal: view.balances[modelData.account_id]
                RowLayout {
                    anchors.fill: parent; anchors.margins: 12; spacing: 12
                    Rectangle {
                        width: 70; height: 22; radius: 11
                        color: modelData.kind === "private" ? "#2d1f3b" : "#1f2d3b"
                        Label { anchors.centerIn: parent; text: modelData.kind === "private" ? "PRIVATE" : "PUBLIC"; color: modelData.kind === "private" ? "#c58af9" : "#8ab4f8"; font.pixelSize: 11 }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true; spacing: 2
                        Label { text: Fmt.label(modelData); color: "#e8eaed"; font.bold: true }
                        Label { text: modelData.account_id; color: "#9aa0a6"; font.pixelSize: 11; elide: Text.ElideMiddle; Layout.fillWidth: true }
                    }
                    ColumnLayout {
                        spacing: 2
                        Label { Layout.alignment: Qt.AlignRight; color: "#e8eaed"; font.pixelSize: 16
                                text: bal ? Fmt.toDisplay(bal.balance) : (modelData.native_balance ? Fmt.toDisplay(modelData.native_balance) : "—") }
                        Label { Layout.alignment: Qt.AlignRight; color: "#9aa0a6"; font.pixelSize: 11
                                text: bal ? (bal.source === "cached" ? "offline (cached)" : bal.source === "local" ? "private (local)" : "live") : "" }
                    }
                    Button { text: "Use"; onClicked: store.call({ op: "set_active", account: modelData.account_id }, function () { store.refresh() }) }
                }
            }
        }
    }
}
