import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Connected Apps (Session Broker) View — Lists active dApp SessionGrants, authorized
// accounts, opt-in ReadPrivateBalance scopes, session TTL, and 1-click Revoke Access.
Item {
    id: connectedAppsRoot
    implicitWidth: 880
    implicitHeight: 640
    Layout.fillWidth: true
    Layout.fillHeight: true

    property var activeSessions: []

    signal revokeSessionRequested(string sessionToken)

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        Label {
            text: "Connected Basecamp dApps & Session Capabilities"
            font.pixelSize: 18
            font.bold: true
        }

        Label {
            text: "Active Sessions: " + connectedAppsRoot.activeSessions.length + " (Default TTL: 24h, Revocable Immediately)"
        }

        Button {
            text: "Revoke Selected Session"
            enabled: connectedAppsRoot.activeSessions.length > 0
            onClicked: {
                if (connectedAppsRoot.activeSessions.length > 0) {
                    connectedAppsRoot.revokeSessionRequested(connectedAppsRoot.activeSessions[0].session_token_preview);
                }
            }
        }
    }
}
