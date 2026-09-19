import java.util.ArrayList;

public class Main {
    static class BankException extends Exception {
        BankException(String m) { super(m); }
    }

    static class InsufficientFunds extends BankException {
        long shortBy;
        InsufficientFunds(String account, long shortBy) {
            super(account + " is short by " + shortBy);
            this.shortBy = shortBy;
        }
    }

    static class AccountFrozen extends BankException {
        AccountFrozen(String account) { super(account + " is frozen"); }
    }

    static class TransferFailed extends Exception {
        TransferFailed(String m, Exception cause) { super(m, cause); }
    }

    static class Account {
        String id;
        long balance;
        boolean frozen = false;

        Account(String id, long balance) {
            this.id = id;
            this.balance = balance;
        }

        void withdraw(long amount) throws BankException {
            if (frozen) {
                throw new AccountFrozen(id);
            }
            if (amount > balance) {
                throw new InsufficientFunds(id, amount - balance);
            }
            balance -= amount;
        }

        void deposit(long amount) throws BankException {
            if (frozen) {
                throw new AccountFrozen(id);
            }
            if (amount <= 0) {
                throw new IllegalArgumentException("deposit must be positive: " + amount);
            }
            balance += amount;
        }
    }

    static class Bank {
        ArrayList<String> audit = new ArrayList<>();
        int operations = 0;

        void transfer(Account from, Account to, long amount) throws TransferFailed {
            operations++;
            try {
                from.withdraw(amount);
                try {
                    to.deposit(amount);
                } catch (BankException e) {
                    from.balance += amount;
                    audit.add("rolled back " + amount + " to " + from.id);
                    throw e;
                }
                audit.add("moved " + amount + " " + from.id + "->" + to.id);
            } catch (InsufficientFunds | AccountFrozen e) {
                throw new TransferFailed("transfer of " + amount + " failed", e);
            } catch (BankException e) {
                throw new TransferFailed("unexpected bank error", e);
            } finally {
                audit.add("op " + operations + " checked");
            }
        }
    }

    static void attempt(Bank bank, Account a, Account b, long amount) {
        try {
            bank.transfer(a, b, amount);
            System.out.println("ok: " + a.id + "=" + a.balance + " " + b.id + "=" + b.balance);
        } catch (TransferFailed e) {
            System.out.println("failed: " + e.getMessage());
            Throwable cause = e.getCause();
            if (cause != null) {
                System.out.println("  because: " + cause.getMessage());
            }
        } catch (IllegalArgumentException e) {
            System.out.println("rejected: " + e.getMessage());
        }
    }

    public static void main(String[] args) {
        Bank bank = new Bank();
        Account alice = new Account("alice", 100);
        Account bob = new Account("bob", 20);
        Account carol = new Account("carol", 0);
        attempt(bank, alice, bob, 30);
        attempt(bank, bob, carol, 80);
        carol.frozen = true;
        attempt(bank, alice, carol, 10);
        attempt(bank, alice, bob, 0);
        carol.frozen = false;
        attempt(bank, carol, alice, 5);
        System.out.println("alice=" + alice.balance + " bob=" + bob.balance + " carol=" + carol.balance);
        for (String line : bank.audit) {
            System.out.println(line);
        }
    }
}
