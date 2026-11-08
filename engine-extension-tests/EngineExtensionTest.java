// SPDX-License-Identifier: GPL-3.0-only
import java.io.IOException;
import java.lang.reflect.Proxy;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;
import net.sourceforge.argparse4j.inf.Namespace;
import org.asamk.signal.commands.GetClientStateCommand;
import org.asamk.signal.commands.SendReceiptCommand;
import org.asamk.signal.commands.SendTypingCommand;
import org.asamk.signal.commands.exceptions.CommandException;
import org.asamk.signal.manager.Manager;
import org.asamk.signal.manager.api.Configuration;
import org.asamk.signal.manager.api.SendMessageResults;
import org.asamk.signal.output.JsonWriter;

public final class EngineExtensionTest {
    static Manager manager(boolean enabled, AtomicInteger typingCalls, AtomicInteger readCalls) {
        return (Manager) Proxy.newProxyInstance(Manager.class.getClassLoader(), new Class<?>[]{Manager.class}, (proxy, method, args) -> {
            return switch (method.getName()) {
                case "getConfiguration" -> new Configuration(Optional.of(enabled), Optional.of(false), Optional.of(enabled), Optional.of(false));
                case "getSelfNumber" -> "+12025550999";
                case "sendTypingMessage" -> { typingCalls.incrementAndGet(); throw new IOException("test transport sentinel"); }
                case "sendReadReceipt" -> { readCalls.incrementAndGet(); yield new SendMessageResults(42, Map.of()); }
                default -> throw new AssertionError("Unexpected engine mutation or operation: " + method.getName());
            };
        });
    }
    public static void main(String[] args) throws Exception {
        var typing = new AtomicInteger(); var read = new AtomicInteger(); var result = new AtomicReference<Object>();
        JsonWriter writer = result::set;
        var disabled = manager(false, typing, read);
        new GetClientStateCommand().handleCommand(new Namespace(Map.of()), disabled, writer);
        var state = (Map<?, ?>) result.get();
        if (!Boolean.FALSE.equals(state.get("typingIndicators")) || !Boolean.FALSE.equals(state.get("readReceipts"))) throw new AssertionError("Phone settings were not preserved");
        new SendTypingCommand().handleCommand(new Namespace(Map.of("recipient", List.of("+12025550123"))), disabled, writer);
        if (typing.get() != 0 || !Boolean.TRUE.equals(((Map<?, ?>)result.get()).get("privacySuppressed"))) throw new AssertionError("Typing leaked while the phone preference was off");
        try {
            new SendTypingCommand().handleCommand(new Namespace(Map.of("recipient", List.of("+12025550123"), "stop", true)), disabled, writer);
        } catch (CommandException expected) {
            if (!expected.getMessage().contains("test transport sentinel")) throw expected;
        }
        if (typing.get() != 1) throw new AssertionError("STOP must still clear a previously active indicator");
        try {
            new SendTypingCommand().handleCommand(new Namespace(Map.of("recipient", List.of("+12025550123"))), manager(true, typing, read), writer);
        } catch (CommandException expected) {
            if (!expected.getMessage().contains("test transport sentinel")) throw expected;
        }
        if (typing.get() != 2) throw new AssertionError("Enabled typing did not reach the engine transport");
        new SendReceiptCommand().handleCommand(new Namespace(Map.of("recipient", "+12025550123", "target-timestamp", List.of(42L))), disabled, writer);
        if (read.get() != 1 || !Boolean.TRUE.equals(((Map<?, ?>)result.get()).get("privacySuppressed"))) throw new AssertionError("Privacy-suppressed receipt should complete after the linked read sync");
        var transcript = new org.asamk.signal.manager.api.MessageEnvelope.Sync.Sent(123L, 321L, Optional.empty(), java.util.Set.of(), Optional.empty(), Optional.empty(), Optional.empty());
        var syncType = Class.forName("org.asamk.signal.json.JsonSyncDataMessage");
        var convert = syncType.getDeclaredMethod("from", transcript.getClass(), Manager.class);
        convert.setAccessible(true);
        var converted = convert.invoke(null, transcript, disabled);
        var expiry = syncType.getDeclaredMethod("expirationStartTimestamp");
        expiry.setAccessible(true);
        if (!Long.valueOf(321L).equals(expiry.invoke(converted))) throw new AssertionError("Phone expiry start was lost in serialization");
        var inline = org.asamk.signal.manager.util.DataURI.of("data:text/plain;filename=a%2Bb%3B%20%F0%9F%98%83.txt;base64,cGF5bG9hZA==");
        if (!"a+b; 😃.txt".equals(inline.parameter().get("filename")) || !java.util.Arrays.equals(inline.data(), "payload".getBytes(java.nio.charset.StandardCharsets.UTF_8))) throw new AssertionError("Attachment filename/data URI did not round trip through the actual engine parser");
        for (var safety : List.of("bad", "1234567890".repeat(6))) {
            try {
                new org.asamk.signal.commands.SetIdentityUnverifiedCommand().handleCommand(new Namespace(Map.of("recipient", "+12025550123", "verified-safety-number", safety)), disabled, writer);
                throw new AssertionError("Unverify must fail closed for malformed safety numbers and unsupported managers");
            } catch (org.asamk.signal.commands.exceptions.UserErrorException expected) {}
        }
        System.out.println("Engine extension: five privacy checks, phone expiry serialization, attachment URI round trip, and two fail-closed unverify checks passed");
    }
}
