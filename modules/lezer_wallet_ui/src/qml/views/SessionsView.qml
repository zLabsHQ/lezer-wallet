import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Format.js" as Fmt

// Connected Basecamp modules. The origin shown is the host-authenticated module name.
Item {
    property var store
    ColumnLayout {
        anchors.fill: parent
        spacing: 8
        Label { text: "Connected apps"; color: "#e8eaed"; font.bold: true }
        ListView {
            Layout.fillWidth: true; Layout.fillHeight: true
            clip: true; spacing: 6
            model: store.sessions
            delegate: Rectangle {
                width: ListView.view.width; height: 60; radius: 8; color: "#1b1f27"
                RowLayout {
                    anchors.fill: parent; anchors.margins: 10
                    ColumnLayout {
                        Layout.fillWidth: true; spacing: 2
                        Label { text: modelData.dapp_name + "  (module: " + modelData.dapp_origin + ")"; color: "#e8eaed" }
                        Label { text: modelData.authorized_accounts.map(Fmt.shortId).join(", ") + "  ·  expires " + new Date(modelData.expires_at * 1000).toLocaleString(); color: "#9aa0a6"; font.pixelSize: 11 }
                    }
                    Button {
                        text: "Disconnect"
                        onClicked: store.call({ op: "revoke_session", session_id: modelData.session_id }, function () { store.refresh() })
                    }
                }
            }
            Label { anchors.centerIn: parent; visible: store.sessions.length === 0; text: "No connected apps"; color: "#9aa0a6" }
        }
    }
}
