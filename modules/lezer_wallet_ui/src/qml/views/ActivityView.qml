import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Format.js" as Fmt

Item {
    property var store
    ListView {
        anchors.fill: parent
        clip: true; spacing: 6
        model: store.activity
        delegate: Rectangle {
            width: ListView.view.width; height: 56; radius: 8; color: "#1b1f27"
            RowLayout {
                anchors.fill: parent; anchors.margins: 10; spacing: 12
                ColumnLayout {
                    Layout.fillWidth: true; spacing: 2
                    Label { text: modelData.kind.replace("_", " ") + "  ·  " + Fmt.toDisplay(modelData.amount); color: "#e8eaed" }
                    Label { text: Fmt.shortId(modelData.from_account) + " → " + Fmt.shortId(modelData.to_account) + "   tx " + modelData.tx_hash.slice(0, 16) + "…"; color: "#9aa0a6"; font.pixelSize: 11 }
                }
                Label {
                    color: modelData.status === "included" ? "#81c995" : "#fdd663"
                    text: modelData.status === "included" ? "block #" + modelData.block_id : "submitted"
                }
            }
        }
        Label { anchors.centerIn: parent; visible: store.activity.length === 0; text: "No transactions yet"; color: "#9aa0a6" }
    }
}
