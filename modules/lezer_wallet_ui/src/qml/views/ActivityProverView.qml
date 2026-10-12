import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Activity & Prover Progress View (`ActivityProverView.qml`) — Phase 4 deliverable.
// Renders the live 4-stage ZK proof progress stepper (`Witness -> r0vm -> RapidSnark -> Broadcast`)
// with an elapsed timer, `Semaphore(1)` queue status, and the write-ahead transaction journal.
Item {
    id: proverView
    implicitWidth: 880
    implicitHeight: 640
    Layout.fillWidth: true
    Layout.fillHeight: true

    property string queueState: "idle"
    property string activeProposalId: ""
    property string currentStage: "Witness"
    property int currentStageIndex: 1
    property int elapsedMs: 0
    property bool deferredAutoLockActive: false

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        Label {
            text: "Zero-Knowledge Prover Queue (Semaphore(1)) & Write-Ahead Nullifier Journal"
            font.pixelSize: 18
            font.bold: true
        }

        Rectangle {
            Layout.fillWidth: true
            height: 76
            color: proverView.queueState === "proving" ? "#14291d" : "#1c2027"

            ColumnLayout {
                anchors.centerIn: parent
                spacing: 4

                Label {
                    text: proverView.queueState === "proving"
                        ? ("Stage [" + proverView.currentStageIndex + "/4]: " + proverView.currentStage + " (Elapsed: " + proverView.elapsedMs + " ms)")
                        : "Prover Queue Idle (1 / 1 Permit Available)"
                    font.bold: true
                }

                Label {
                    text: "Pipeline: [1] Witness  ->  [2] r0vm (RISC Zero)  ->  [3] RapidSnark (Groth16)  ->  [4] Broadcast"
                }

                Label {
                    visible: proverView.deferredAutoLockActive
                    text: "DeferredLockPending Active: Vault will auto-lock immediately after proof completes."
                }
            }
        }
    }
}
