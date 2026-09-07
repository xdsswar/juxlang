import java.util.ArrayList;

public class Main {
    static class Account {
        private String owner;
        private int balance;
        Account(String owner, int opening) {
            this.owner = owner;
            this.balance = opening;
        }
        String getOwner() { return this.owner; }
        int getBalance() { return this.balance; }
        boolean withdraw(int amount) {
            if (amount > this.balance) { return false; }
            this.balance = this.balance - amount;
            return true;
        }
        void deposit(int amount) { this.balance = this.balance + amount; }
    }
    static class Bank {
        private ArrayList<Account> accounts;
        Bank() { this.accounts = new ArrayList<>(); }
        void open(Account a) { this.accounts.add(a); }
        int total() {
            int t = 0;
            for (Account a : this.accounts) { t = t + a.getBalance(); }
            return t;
        }
        boolean transfer(int from, int to, int amount) {
            Account src = this.accounts.get(from);
            Account dst = this.accounts.get(to);
            if (!src.withdraw(amount)) { return false; }
            dst.deposit(amount);
            return true;
        }
        int count() { return this.accounts.size(); }
        Account at(int i) { return this.accounts.get(i); }
    }

    public static void main(String[] args) {
        Bank bank = new Bank();
        bank.open(new Account("ada", 100));
        bank.open(new Account("bo", 50));
        bank.open(new Account("cy", 0));
        System.out.println(bank.count());
        System.out.println(bank.total());
        System.out.println(bank.transfer(0, 2, 40));
        System.out.println(bank.at(0).getBalance());
        System.out.println(bank.at(2).getBalance());
        System.out.println(bank.total());
        System.out.println(bank.transfer(2, 1, 1000));
        System.out.println(bank.total());
        for (int i = 0; i < bank.count(); i++) {
            System.out.println(bank.at(i).getOwner() + "=" + bank.at(i).getBalance());
        }
    }
}
