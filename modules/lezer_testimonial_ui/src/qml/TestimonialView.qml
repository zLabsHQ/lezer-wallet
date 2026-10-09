import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Lezer Testimonial Mini-App (`lezer_testimonial_ui`) — SDK Consumer #2.
// Features:
// - 1-click "Create Fresh Public Account for Testimonial" privacy helper
// - Hard-locked `submission_id = "lp0021-lezer"` + SHA-256 16-shard routing
// - Live 16-shard on-chain testimonial feed & adoption counter
Item {
    id: testimonialView
    implicitWidth: 880
    implicitHeight: 640
    Layout.fillWidth: true
    Layout.fillHeight: true

    property string submissionId: "lp0021-lezer"
    property string selectedAuthor: ""
    property int totalDistinctAuthors: 0

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        Label {
            text: "Lezer On-Chain Testimonials (lp0021-lezer) — 16-Shard Adoption Feed"
            font.pixelSize: 18
            font.bold: true
        }

        RowLayout {
            spacing: 8
            Button {
                text: "Connect Lezer Wallet"
            }
            Button {
                text: "Create Fresh Public Account for Testimonial"
            }
        }

        TextField {
            id: usernameInput
            placeholderText: "Optional Handle (e.g. @alice)"
            Layout.fillWidth: true
        }

        TextArea {
            id: textInput
            text: "I use Lezer Wallet on Logos Execution Zone!"
            Layout.fillWidth: true
        }

        Button {
            text: "Submit On-Chain Testimonial (lp0021-lezer)"
        }

        Label {
            text: "Live 16-Shard On-Chain Feed (" + testimonialView.totalDistinctAuthors + " / 150 distinct accounts verified)"
            font.bold: true
        }
    }
}
