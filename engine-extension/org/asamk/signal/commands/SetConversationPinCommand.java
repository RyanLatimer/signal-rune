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

/** Queue an idempotent account-storage mutation; a queue acknowledgement is not a remote ack. */
public final class SetConversationPinCommand implements JsonRpcLocalCommand {
    @Override public String getName(){return "setConversationPin";}
    @Override public void attachToSubparser(Subparser p){
        p.help("Synchronize a conversation pin through encrypted account storage.");
        p.addArgument("--operation-id").type(long.class).required(true);
        var target=p.addMutuallyExclusiveGroup().required(true);target.addArgument("--recipient");target.addArgument("--group-id");
        p.addArgument("--pinned").type(Boolean.class).required(true);
    }
    @Override public void handleCommand(Namespace args,Manager manager,OutputWriter out) throws CommandException {
        try {
            var id=args.getLong("operation-id");var pinned=args.getBoolean("pinned");
            if(id==null || pinned==null) throw new IllegalArgumentException();
            var result=RunePins.set(manager,id,args.getString("recipient"),args.getString("group-id"),pinned);
            switch(out){case JsonWriter json->json.write(result);case PlainTextWriter text->text.println("Pin queued: {}",id);}
        } catch(Exception e){throw new UserErrorException("Pin synchronization could not be queued; existing account storage was preserved.");}
    }
}
