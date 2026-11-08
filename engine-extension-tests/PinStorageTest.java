// SPDX-License-Identifier: GPL-3.0-only
import java.nio.file.Files;
import java.sql.DriverManager;
import java.util.List;
import org.asamk.signal.manager.helper.RunePins;
import org.whispersystems.signalservice.internal.storage.protos.AccountRecord;
import org.whispersystems.signalservice.internal.storage.protos.AccountRecord.PinnedConversation;
import okio.ByteString;

/** No Signal account or network: real Wire records and the engine's SQLite driver. */
public final class PinStorageTest {
    private static PinnedConversation person(String uuid,String phone) {
        return new PinnedConversation.Builder().contact(new PinnedConversation.Contact.Builder().serviceId(uuid).e164(phone).build()).build();
    }
    public static void main(String[] args) throws Exception {
        var alice=person("11111111-1111-4111-8111-111111111111","+12025550123");
        var bob=person("22222222-2222-4222-8222-222222222222","+12025550456");
        var reassigned=person("33333333-3333-4333-8333-333333333333","+12025550123");
        if(RunePins.same(alice,reassigned)) throw new AssertionError("Reassigned phone numbers must not merge two different identities");
        var unknown=new PinnedConversation.Builder().addUnknownFields(ByteString.decodeHex("f80701")).build();
        var group=new PinnedConversation.Builder().groupMasterKey(ByteString.of(new byte[32])).build();
        var file=Files.createTempFile("signal-rune-pin-test-",".db");
        try {
            String url="jdbc:sqlite:"+file;
            var add=new RunePins.Intent(100,alice,true);
            try(var db=DriverManager.getConnection(url)) {
                if(RunePins.exists(db)) throw new AssertionError("Read-only discovery must not create a pin table");
                if(!RunePins.overlay(db,List.of(unknown)).equals(List.of(unknown))) throw new AssertionError("Unknown remote records were lost");
                if(RunePins.record(db,add)!=RunePins.RecordStatus.NEW || RunePins.record(db,add)!=RunePins.RecordStatus.PENDING) throw new AssertionError("Repeated operation must be idempotent");
                var remote=new AccountRecord.Builder().typingIndicators(false).readReceipts(false).linkPreviews(false).pinnedConversations(List.of(unknown,bob,group)).addUnknownFields(ByteString.decodeHex("f80701")).build();
                var merged=remote.newBuilder().pinnedConversations(RunePins.overlay(db,remote.pinnedConversations)).build();
                if(merged.typingIndicators || merged.readReceipts || merged.linkPreviews || !merged.unknownFields().equals(remote.unknownFields())) throw new AssertionError("Pin updates changed unrelated phone settings or unknown fields");
                if(!merged.pinnedConversations.equals(List.of(unknown,bob,group,alice))) throw new AssertionError("Concurrent remote pins or unknown records were overwritten");
                if(!RunePins.overlay(db,merged.pinnedConversations).equals(merged.pinnedConversations)) throw new AssertionError("Retry reordered existing pins");
                RunePins.record(db,new RunePins.Intent(101,alice,false));
                if(!RunePins.overlay(db,merged.pinnedConversations).equals(List.of(unknown,bob,group))) throw new AssertionError("Ordered unpin was not applied");
                if(RunePins.acknowledge(db,new byte[]{2},new byte[]{1}) || RunePins.pending(db).size()!=2) throw new AssertionError("An older write acknowledged newer local intents");
            }
            try(var db=DriverManager.getConnection(url)) {
                if(RunePins.pending(db).size()!=2) throw new AssertionError("Queued pin changes did not survive restart");
                if(!RunePins.acknowledge(db,new byte[]{2},new byte[]{2}) || !RunePins.pending(db).isEmpty() || RunePins.record(db,add)!=RunePins.RecordStatus.DONE) throw new AssertionError("Confirmed remote acknowledgement was not durable");
                try {RunePins.record(db,new RunePins.Intent(100,bob,true));throw new AssertionError("Reused operation ID silently changed its payload");}catch(IllegalArgumentException expected) {}
            }
        } finally {Files.deleteIfExists(file);}
        System.out.println("Pin storage: identity isolation, unknown/privacy preservation, concurrent merge, ordered idempotent retries, restart durability, and generation-bound acknowledgements passed");
    }
}
