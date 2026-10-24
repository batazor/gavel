ALTER TABLE auction_process_outbox
    DROP CONSTRAINT auction_process_outbox_event_type_check;

ALTER TABLE auction_process_outbox
    ADD CONSTRAINT auction_process_outbox_event_type_check
    CHECK (
        event_type IN (
            'SettlementCandidateSelected',
            'AuctionUnsold',
            'WinnerConfirmed',
            'AuctionCancelled',
            'BidderOutbid'
        )
    );
