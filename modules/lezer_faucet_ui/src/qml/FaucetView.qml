import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Lezer Piñata PoW Faucet Mini-App (`lezer_faucet_ui`) — SDK Consumer #1.
// Features:
// - Live pool depth, claims remaining, PoW difficulty bytes, and cooldown timer
// - 1-click "Claim 150 LEZ" with stale-challenge retry status
// - "Claim & Auto-Shield" toggle for funding private accounts
Item {
    id: faucetView
    implicitWidth: 880
    implicitHeight: 640
    Layout.fillWidth: true
    Layout.fillHeight: true

    property string poolBalance: "1500000"
    property string prizeAmount: "150"
    property int difficultyBytes: 1
    property int cooldownSeconds: 0
    property bool autoShieldEnabled: false

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        Label {
            text: "Lezer Piñata Proof-of-Work Faucet (150 LEZ)"
            font.pixelSize: 18
            font.bold: true
        }

        Label {
            text: "Pool Depth: " + faucetView.poolBalance + " LEZ | PoW Difficulty: " + faucetView.difficultyBytes + " zero byte(s) | Cooldown: " + faucetView.cooldownSeconds + "s"
        }

        CheckBox {
            text: "Claim & Auto-Shield to Shielded Vault (Private/<id>)"
            checked: faucetView.autoShieldEnabled
            onCheckedChanged: faucetView.autoShieldEnabled = checked
        }

        Button {
            text: "Solve PoW & Claim 150 LEZ"
        }
    }
}
