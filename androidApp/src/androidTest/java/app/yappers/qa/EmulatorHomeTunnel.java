package app.yappers.qa;

import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;

/** Carries local QUIC datagrams over an adb shell pipe, preserving the client's UDP port. */
public final class EmulatorHomeTunnel {
    public static void main(String[] args) throws Exception {
        InetAddress loopback = InetAddress.getByName("127.0.0.1");
        try (DatagramSocket socket = new DatagramSocket(Integer.parseInt(args[0]), loopback)) {
            DataInputStream input = new DataInputStream(System.in);
            DataOutputStream output = new DataOutputStream(System.out);
            Thread responses = new Thread(() -> {
                byte[] data = new byte[65535];
                try {
                    while (true) {
                        int port = input.readUnsignedShort();
                        int length = input.readUnsignedShort();
                        input.readFully(data, 0, length);
                        socket.send(new DatagramPacket(data, length, loopback, port));
                    }
                } catch (IOException done) {
                    socket.close();
                }
            });
            responses.setDaemon(true);
            responses.start();
            output.writeInt(0); // Ready after the socket is bound.
            output.flush();
            byte[] data = new byte[65535];
            while (!socket.isClosed()) {
                DatagramPacket packet = new DatagramPacket(data, data.length);
                try { socket.receive(packet); }
                catch (IOException done) { if (socket.isClosed()) return; throw done; }
                output.writeShort(packet.getPort());
                output.writeShort(packet.getLength());
                output.write(packet.getData(), 0, packet.getLength());
                output.flush();
            }
        }
    }
}
