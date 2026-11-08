// SPDX-License-Identifier: GPL-3.0-only
package org.asamk.signal.commands;
import net.sourceforge.argparse4j.inf.Namespace;
import net.sourceforge.argparse4j.inf.Subparser;
import org.asamk.signal.commands.exceptions.CommandException;
import org.asamk.signal.commands.exceptions.UserErrorException;
import org.asamk.signal.manager.Manager;
import org.asamk.signal.manager.helper.RunePins;
import org.asamk.signal.output.OutputWriter;
import org.asamk.signal.output.JsonWriter;
import org.asamk.signal.output.PlainTextWriter;
import java.util.Map;
public final class SyncConversationPinsCommand implements JsonRpcLocalCommand {
    @Override public String getName(){return "syncConversationPins";}
    @Override public void attachToSubparser(Subparser p){p.help("Retry pending encrypted conversation pin storage writes.");}
    @Override public void handleCommand(Namespace args,Manager manager,OutputWriter out) throws CommandException {
        try {RunePins.schedule(RunePins.context(manager));switch(out){case JsonWriter json->json.write(Map.of("queued",true));case PlainTextWriter text->text.println("Pin sync queued");}}
        catch(Exception e){throw new UserErrorException("Pin synchronization is unavailable in this engine.");}
    }
}
