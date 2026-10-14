import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Worked example Basecamp `ui_qml` dApp consuming `lezer_wallet_provider`:
// 1. Connect Wallet -> 2. Read Authorized Account & Balance -> 3. Propose Decoded Transaction
Item {
    id: starterRoot
    implicitWidth: 800
    implicitHeight: 560
    Layout.fillWidth: true
    Layout.fillHeight: true

    LezerProvider {
        id: lezer
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        Label {
            text: "Lezer Basecamp dApp Starter Template"
            font.pixelSize: 18
            font.bold: true
        }

        Button {
            text: "1. Connect to Lezer Wallet"
            onClicked: lezer.connectDapp(JSON.stringify({
                moduleName: "lezer_dapp_starter",
                displayName: "Lezer dApp Starter",
                version: "0.1.0"
            }))
        }

        Button {
            text: "2. Propose Sample Native Transfer (10 LEZ)"
            onClicked: lezer.proposeTransaction(JSON.stringify({
                kind: "native_transfer",
                from: "Main Public",
                to: "Main Public",
                amount: "10"
            }))
        }
    }
}
