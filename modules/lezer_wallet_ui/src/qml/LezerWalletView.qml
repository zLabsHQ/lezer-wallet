import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Lezer Wallet UI — Root Basecamp QML container subscribing to Rust `WalletStateSnapshot`.
// Stateless UI renderer: holds zero keys and performs zero local state invention.
Item {
    id: root
    implicitWidth: 920
    implicitHeight: 680
    Layout.fillWidth: true
    Layout.fillHeight: true

    property string lockState: "uninitialized"
    property string sequencerUrl: "https://testnet.lez.logos.co"

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        Label {
            text: "Lezer Wallet (" + root.lockState + ") — " + root.sequencerUrl
            font.bold: true
        }
    }
}
