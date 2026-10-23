import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Format.js" as Fmt

// A Basecamp module asks to connect. The origin is the host-authenticated module name;
// the display name is self-declared by the dApp and shown as secondary information.
Rectangle {
    id: sheet
    property var request
    property var store
    property var selected: ({})
    property var privateRead: ({})
    color: "#cc000000"

    onRequestChanged: { selected = ({}); privateRead = ({}) }
    MouseArea { anchors.fill: parent }

    Rectangle {
        anchors.centerIn: parent
        width: Math.min(parent.width - 48, 560); radius: 12; color: "#1b1f27"
        implicitHeight: col.implicitHeight + 32
        height: implicitHeight
        ColumnLayout {
            id: col
            anchors.fill: parent; anchors.margins: 16; spacing: 10
            Label { text: "Connection request"; color: "#e8eaed"; font.pixelSize: 18; font.bold: true }
            Label {
                Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#e8eaed"
                text: sheet.request ? "Module “" + sheet.request.summary_json.dapp_origin + "” (calls itself “" + sheet.request.dapp_name + "”) wants to see the accounts you choose and propose transactions for your approval." : ""
            }
            Label { text: "Share accounts"; color: "#9aa0a6" }
            Repeater {
                model: sheet.store.accounts
                RowLayout {
                    Layout.fillWidth: true
                    CheckBox {
                        text: (modelData.kind === "private" ? "🔒 " : "") + Fmt.label(modelData) + "  " + Fmt.shortId(modelData.account_id)
                        onToggled: { var s = sheet.selected; s[modelData.account_id] = checked; sheet.selected = s }
                    }
                    Item { Layout.fillWidth: true }
                    CheckBox {
                        visible: modelData.kind === "private" && !!sheet.selected[modelData.account_id]
                        text: "allow balance reads"
                        onToggled: { var p = sheet.privateRead; p[modelData.account_id] = checked; sheet.privateRead = p }
                    }
                }
            }
            Label { Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#9aa0a6"; font.pixelSize: 12
                    text: "The app never receives keys. Private balances stay hidden unless you allow it per account. Every transaction still needs your approval." }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                Button {
                    text: "Reject"
                    onClicked: store.call({ op: "reject", request_id: sheet.request.request_id, reason: "Rejected by user" }, function () { store.refresh() })
                }
                Button {
                    text: "Connect"
                    highlighted: true
                    enabled: Object.keys(sheet.selected).some(function (k) { return sheet.selected[k] })
                    onClicked: {
                        var accts = Object.keys(sheet.selected).filter(function (k) { return sheet.selected[k] })
                        var priv = Object.keys(sheet.privateRead).filter(function (k) { return sheet.privateRead[k] && sheet.selected[k] })
                        store.call({ op: "approve_connect", request_id: sheet.request.request_id, accounts: accts, allow_private_balances: priv },
                                   function () { store.refresh() })
                    }
                }
            }
        }
    }
}
