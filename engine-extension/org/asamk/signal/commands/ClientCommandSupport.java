// SPDX-License-Identifier: GPL-3.0-only
package org.asamk.signal.commands;

import java.util.List;
import java.util.Map;
import org.asamk.signal.output.JsonWriter;
import org.asamk.signal.output.OutputWriter;
import org.asamk.signal.output.PlainTextWriter;

final class ClientCommandSupport {
    private ClientCommandSupport() {}
    static void suppressed(OutputWriter writer, long timestamp) {
        switch (writer) {
            case JsonWriter json -> json.write(Map.of("timestamp", timestamp, "results", List.of(), "privacySuppressed", true));
            case PlainTextWriter text -> text.println("{}", timestamp);
        }
    }
}
