// SPDX-License-Identifier: GPL-3.0-only
package org.asamk.signal.commands;

import java.util.Map;
import net.sourceforge.argparse4j.inf.Namespace;
import net.sourceforge.argparse4j.inf.Subparser;
import org.asamk.signal.manager.Manager;
import org.asamk.signal.output.JsonWriter;
import org.asamk.signal.output.OutputWriter;
import org.asamk.signal.output.PlainTextWriter;

/** Read the engine's phone-synchronized privacy settings without changing them. */
public final class GetClientStateCommand implements JsonRpcLocalCommand {
    @Override public String getName() { return "getClientState"; }
    @Override public void attachToSubparser(Subparser parser) {
        parser.help("Read synchronized privacy settings for Signal Rune (does not modify the account).");
    }
    @Override public void handleCommand(Namespace args, Manager manager, OutputWriter writer) {
        var config = manager.getConfiguration();
        var result = new java.util.HashMap<String,Object>(Map.of(
            "version", 1,
            "readReceipts", config.readReceipts().orElse(true),
            "typingIndicators", config.typingIndicators().orElse(true),
            "linkPreviews", config.linkPreviews().orElse(true),
            "unidentifiedDeliveryIndicators", config.unidentifiedDeliveryIndicators().orElse(false)));
        try {result.put("conversationPins", org.asamk.signal.manager.helper.RunePins.state(manager));}
        catch(Exception ignored) {result.put("conversationPins",Map.of("supported",false));}
        switch (writer) {
            case JsonWriter json -> json.write(result);
            case PlainTextWriter text -> result.forEach((key, value) -> text.println("{}: {}", key, value));
        }
    }
}
