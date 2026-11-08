// SPDX-License-Identifier: GPL-3.0-only
package org.asamk.signal.commands;

import net.sourceforge.argparse4j.inf.Namespace;
import net.sourceforge.argparse4j.inf.Subparser;
import org.asamk.signal.commands.exceptions.CommandException;
import org.asamk.signal.commands.exceptions.UserErrorException;
import org.asamk.signal.manager.Manager;
import org.asamk.signal.manager.api.UnregisteredRecipientException;
import org.asamk.signal.manager.helper.RuneIdentityAccess;
import org.asamk.signal.output.OutputWriter;
import org.asamk.signal.util.CommandUtil;

/** Unverify the exact displayed identity; changing keys makes this operation fail. */
public final class SetIdentityUnverifiedCommand implements JsonRpcLocalCommand {
    @Override public String getName() {return "setIdentityUnverified";}
    @Override public void attachToSubparser(Subparser parser) {
        parser.help("Clear verification only when the expected safety number still matches.");
        parser.addArgument("recipient").required(true);
        parser.addArgument("--verified-safety-number").required(true);
    }
    @Override public void handleCommand(Namespace args, Manager manager, OutputWriter output) throws CommandException {
        var safety = args.getString("verified-safety-number");
        if (safety == null || !safety.matches("[0-9]{60}")) throw new UserErrorException("Expected a 60 digit safety number");
        var recipient = CommandUtil.getSingleRecipientIdentifier(args.getString("recipient"), manager.getSelfNumber());
        try {
            if (!RuneIdentityAccess.unverify(manager, recipient, safety)) throw new UserErrorException("Identity changed; compare its current safety number before retrying.");
        } catch (ReflectiveOperationException | IllegalArgumentException | UnregisteredRecipientException e) {
            throw new UserErrorException("The current identity could not be checked; verification was not changed.");
        }
    }
}
