package app.yappers

import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.nio.file.attribute.PosixFilePermissions
import java.nio.channels.FileChannel
import java.nio.ByteBuffer
import java.util.HexFormat
import javax.swing.JFileChooser
import javax.swing.JOptionPane
import javax.swing.SwingUtilities

/** Native file dialogs only when requested by the user; cryptography and file IO stay off Swing. */
internal class DesktopRecoveryFiles : RecoveryFiles {
    private val maximum = 16 * 1024 * 1024 + 65536
    override fun saveRecovery(ciphertext: String, onResult: (String?, String?) -> Unit) {
        SwingUtilities.invokeLater {
            val picker = JFileChooser().apply { dialogTitle = "Save encrypted Yap backup"; selectedFile = java.io.File("Yap-Recovery.age") }
            if (picker.showSaveDialog(null) != JFileChooser.APPROVE_OPTION) { onResult(null, null); return@invokeLater }
            val target = picker.selectedFile.toPath().toAbsolutePath()
            if (Files.exists(target) && JOptionPane.showConfirmDialog(null, "Replace this existing backup?", "Save backup", JOptionPane.YES_NO_OPTION) != JOptionPane.YES_OPTION) { onResult(null, null); return@invokeLater }
            worker(onResult) {
                require(ciphertext.length <= maximum * 2) { "Backup exceeds file capacity." }
                val bytes = HexFormat.of().parseHex(ciphertext)
                val temporary = Files.createTempFile(target.parent, ".yap-recovery-", ".tmp", PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------")))
                try {
                    FileChannel.open(temporary, StandardOpenOption.WRITE).use { channel ->
                        val buffer = ByteBuffer.wrap(bytes); while (buffer.hasRemaining()) channel.write(buffer); channel.force(true)
                    }
                    Files.move(temporary, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
                    FileChannel.open(target.parent, StandardOpenOption.READ).use { it.force(true) }
                } finally { Files.deleteIfExists(temporary) }
                "saved"
            }
        }
    }
    override fun loadRecovery(onResult: (String?, String?) -> Unit) {
        SwingUtilities.invokeLater {
            val picker = JFileChooser().apply { dialogTitle = "Choose encrypted Yap backup"; fileFilter = javax.swing.filechooser.FileNameExtensionFilter("Encrypted backup", "age") }
            if (picker.showOpenDialog(null) != JFileChooser.APPROVE_OPTION) { onResult(null, null); return@invokeLater }
            val source = picker.selectedFile.toPath()
            worker(onResult) {
                require(Files.size(source) <= maximum) { "Backup exceeds file capacity." }
                val bytes = Files.newInputStream(source).use { it.readNBytes(maximum + 1) }
                require(bytes.size <= maximum) { "Backup exceeds file capacity." }
                HexFormat.of().formatHex(bytes)
            }
        }
    }
    private fun worker(onResult: (String?, String?) -> Unit, task: () -> String) {
        Thread({
            val result = runCatching(task)
            SwingUtilities.invokeLater { result.fold({ onResult(it, null) }, { onResult(null, it.message ?: "Could not access this backup file.") }) }
        }, "yap-recovery-file").apply { isDaemon = true; start() }
    }
}
