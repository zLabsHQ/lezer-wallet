import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Settings & NetworkProfile View — Switch between lez-testnet-0.4, lez-testnet-0.3,
// or custom LEZ_SEQUENCER_URL, configure Logos Storage (Codex) URL, Trusted Verifiers,
// and 15-minute Auto-Lock timeout.
Item {
    id: settingsRoot
    implicitWidth: 880
    implicitHeight: 640
    Layout.fillWidth: true
    Layout.fillHeight: true

    property string profileId: "lez-testnet-0.4"
    property string sequencerUrl: "https://testnet.lez.logos.co"
    property string storageNodeUrl: "http://127.0.0.1:8080"
    property int autoLockSeconds: 900

    signal saveSettingsRequested(string profileId, string sequencerUrl, string storageNodeUrl, int autoLockSeconds)
    signal pushBackupRequested(string storageNodeUrl)
    signal pullBackupRequested(string cid, string storageNodeUrl)

    ColumnLayout {
        anchors.fill: parent
        spacing: 10

        Label {
            text: "Network Profile, Logos Storage (Codex) Backup & Auto-Lock Settings"
            font.pixelSize: 18
            font.bold: true
        }

        TextField {
            id: seqField
            text: settingsRoot.sequencerUrl
            placeholderText: "Sequencer JSON-RPC URL (AllowedEndpointsGuard enforced)"
            Layout.fillWidth: true
        }

        TextField {
            id: storageField
            text: settingsRoot.storageNodeUrl
            placeholderText: "Logos Storage (Codex) Node URL"
            Layout.fillWidth: true
        }

        RowLayout {
            spacing: 8
            Button {
                text: "Save Network & Auto-Lock (15m)"
                onClicked: settingsRoot.saveSettingsRequested(
                    settingsRoot.profileId,
                    seqField.text,
                    storageField.text,
                    settingsRoot.autoLockSeconds
                )
            }
            Button {
                text: "Push Encrypted Vault Backup (CID)"
                onClicked: settingsRoot.pushBackupRequested(storageField.text)
            }
        }
    }
}
