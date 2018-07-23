#[derive(Debug, PartialEq)]
pub enum AccountCmd {
    New(NewAccount),
    List(ListAccounts),
}

#[derive(Debug, PartialEq)]
pub struct ListAccounts;

#[derive(Debug, PartialEq)]
pub struct NewAccount;
