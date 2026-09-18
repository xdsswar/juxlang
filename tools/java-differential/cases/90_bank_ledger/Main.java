import java.util.ArrayList;
import java.util.HashMap;

public class Main {
    interface Account {
        String id();
        double balance();
        void deposit(double amount);
        void withdraw(double amount) throws BankError;
    }

    static class BankError extends Exception {
        BankError(String message) { super(message); }
    }

    static class InsufficientFunds extends BankError {
        InsufficientFunds(String id, double wanted) {
            super("insufficient funds in " + id + ": wanted " + wanted);
        }
    }

    static class UnknownAccount extends BankError {
        UnknownAccount(String id) { super("no account " + id); }
    }

    static abstract class BaseAccount implements Account {
        protected String accountId;
        protected double funds;

        BaseAccount(String accountId, double opening) {
            this.accountId = accountId;
            this.funds = opening;
        }

        public String id() { return accountId; }
        public double balance() { return funds; }
        public void deposit(double amount) { funds = funds + amount; }

        public abstract double overdraft();

        public void withdraw(double amount) throws BankError {
            if (funds - amount < -overdraft()) {
                throw new InsufficientFunds(accountId, amount);
            }
            funds = funds - amount;
        }
    }

    static class Savings extends BaseAccount {
        Savings(String id, double opening) { super(id, opening); }
        public double overdraft() { return 0.0; }
        void addInterest(double rate) { funds = funds + funds * rate; }
    }

    static class Checking extends BaseAccount {
        Checking(String id, double opening) { super(id, opening); }
        public double overdraft() { return 100.0; }
    }

    static class Bank {
        private HashMap<String, Account> accounts = new HashMap<>();
        private ArrayList<String> ledger = new ArrayList<>();

        void open(Account a) {
            accounts.put(a.id(), a);
            ledger.add("open " + a.id() + " " + a.balance());
        }

        Account find(String id) throws BankError {
            if (!accounts.containsKey(id)) {
                throw new UnknownAccount(id);
            }
            return accounts.get(id);
        }

        void transfer(String from, String to, double amount) {
            try {
                Account a = find(from);
                Account b = find(to);
                a.withdraw(amount);
                b.deposit(amount);
                ledger.add("ok " + from + "->" + to + " " + amount);
            } catch (InsufficientFunds e) {
                ledger.add("refused: " + e.getMessage());
            } catch (BankError e) {
                ledger.add("error: " + e.getMessage());
            }
        }

        void report() {
            for (String line : ledger) {
                System.out.println(line);
            }
        }
    }

    public static void main(String[] args) {
        Bank bank = new Bank();
        Savings s = new Savings("S1", 500.0);
        Checking c = new Checking("C1", 50.0);
        bank.open(s);
        bank.open(c);

        bank.transfer("S1", "C1", 200.0);
        bank.transfer("C1", "S1", 300.0);
        bank.transfer("C1", "S1", 100.0);
        bank.transfer("S1", "X9", 10.0);
        bank.transfer("S1", "C1", 400.0);

        s.addInterest(0.5);
        bank.report();
        System.out.println("S1 " + s.balance());
        System.out.println("C1 " + c.balance());
    }
}
